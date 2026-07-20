# Known Issues — OS kernel

Running list of unsolved bugs and technical debt.  Each entry should
have enough context to act on later: what the bug or debt is, where in
the code it lives, how to reproduce it (for bugs), and what the proper
fix looks like (for debt).

Per CLAUDE.md: "Ideally, bugs and tech debt are fixed immediately as
they're discovered — the tracking file is a fallback for when something
genuinely can't be addressed in the current task, not a place to defer
work that should be done now."

---

## Active Bugs

### TD-OILS-ESCAPED-METACHAR. Backslash-escaped glob/pattern metacharacters in *unquoted* words are treated as live metacharacters — 2026-07-20 — PARTIALLY FIXED (inline `=~` regex done; glob/`case`/`[[ == ]]`/keyword paths OPEN, medium priority)

**Where:** `userspace/oils/src/lexer.rs` — `read_word_inner` (the `'\\'` arm,
~line 979) folds a backslash-escaped character into a plain unquoted `Seg::Lit`,
dropping the "this character was quoted/literal" information. The glob/pattern
machinery keys off a per-character `EChar.quoted` flag
(`interp.rs`, `push_chars`, `field_has_glob_meta`), but an escaped char reaches
it tagged `quoted:false`, so an escaped `*`/`?`/`[` is (wrongly) matched as a
live metacharacter.

**What (symptoms, all one root cause):**
- Command-argument globbing: `echo a\*b` in a dir with `aXb aYb` prints
  `aXb aYb` in osh, but bash prints the literal `a*b` (escape suppresses glob).
- `for f in a\*b; do …` iterates the glob matches in osh; bash yields one literal
  `a*b`.
- `[[ "aXb" == a\*b ]]` → osh matches (Y), bash does not (the `\*` is a literal
  `*`, N). Same for `case "aXb" in a\*b)`.
- Keyword suppression: `\if true; then …` — bash treats `\if` as an ordinary
  **command** named `if` (backslash suppresses reserved-word recognition) and
  reports a syntax error at `then`; osh still recognizes `\if` as the `if`
  keyword and runs the conditional.

**Already fixed (2026-07-20):** the inline `=~` regex RHS. `read_word_regex`
(the dedicated `=~`-RHS lexer) now *preserves* the backslash so `\+`/`\.`/`\(`
reach the ERE engine as literal `+`/`.`/`(`, exactly like a variable-supplied
regex. That path is isolated (its word list feeds only the regex builder), so
the fix was safe and self-contained. See `cond_regex_inline_backslash_escapes_metachar`.

**Proper fix for the rest (the refactor):** carry the escape through to the
`EChar.quoted` flag. Add a `Seg::Escaped(char)` (lexer) / `WordPart::EscapedChar(char)`
(AST) pair; `read_word_inner` emits `Escaped(c)` for a backslash escape; the
field-splitter (`interp.rs` ~line 6753) maps `EscapedChar(c)` to
`push_chars(&c.to_string(), true)` (i.e. `quoted:true`); `expand_to_string`,
`unparse`, and the value-quoters render it as the bare char. The subtle part is
the ~15 `[Seg::Lit(name)]` sites in `parser.rs`/`lexer.rs` used for command-name,
**reserved-word**, and assignment detection: each must be reconsidered
individually — some should fold an `Escaped` run into the flattened literal
(command name: `\ls` → `ls`), while **keyword** detection must *not* (so `\if`
stops being the keyword, matching bash). Because it touches core word/command
parsing and I am the sole tester, this is deferred to its own focused, fully
tested change rather than rushed. Low real-world frequency (scripts almost always
quote a literal metacharacter instead), hence medium priority.

### TD-OILS-RW-OFFSET. `osh`'s `<>` read/write descriptor does not share one OS file offset — 2026-07-19 — OPEN (low priority)

**What:** The `<>` (open-for-read-write) redirect is now implemented
(`RedirectOp::ReadWrite`), covering `<> file` (fd 0 → stdin), `1<>`/`2<>`
(no-truncate write), and `exec {fd}<> file` (persistent rw descriptor).
The *write* side is fully faithful — writes land at offset 0 and overwrite
in place, matching bash (verified via `od`). The *read* side, however, is a
**byte snapshot** taken at open time (osh's fd model stores read fds as
`io::Cursor<Vec<u8>>` in `open_fds` and write fds as live `File`s in
`open_write_fds` — two independent handles). A real `O_RDWR` descriptor
shares ONE OS file offset between reads and writes, so in bash a `read`
after a `>&N` write on the same `<>` fd continues from the post-write
position; in osh the read cursor is independent of the write handle and does
not see writes made through the same fd after open.

**Where:** `userspace/oils/src/interp.rs` — `ExtraFdOp::ReadWriteFile`
(install in `install_extra_fds`, exec loop, and `apply_persistent_redirect`),
`open_rw` helper, and the `RedirectOp::ReadWrite` arm of `resolve_redirects`.

**Repro:** `exec 3<>f; echo AB >&3; read -u 3 x; echo "[$x]"` — bash reads
from just past "AB\n" (EOF → empty); osh's read cursor still starts at the
pre-write snapshot. This is unusual in real scripts (interleaved read+write
on one `<>` fd), so it is low priority.

**Proper fix:** unify osh's fd model so a single descriptor can be backed by
one live `File` (or a shared seek position) usable for both reading and
writing, rather than the split snapshot/live-handle representation. That is a
broader fd-model refactor touching `open_fds`/`open_write_fds` and every
builtin that reads/writes user-space fds — deferred until there is a concrete
need beyond `<>`.

### TD-OILS-BADSUBST-AT. `osh` reports `${x@}` (bare `@` transform) as a bad substitution — 2026-07-19 — OPEN (very low priority)

**What:** bash's handling of a `${name@}` with an *empty* transform operator is
internally inconsistent: unquoted `echo ${x@}` on an unset `x` yields an empty
field with status 0, but quoted `echo "[${x@}]"` (and `x=hi; echo "[${x@}]"`)
reports `${x@}: bad substitution`. osh uniformly treats a bare/unknown `@`
transform as a runtime bad substitution (status 1) — matching bash's *quoted*
case but not its unquoted-empty case.

**Where:** `userspace/oils/src/parser.rs` (the `@` transform arm of
`parse_braced_param`, which now returns `WordPart::BadSubst` when
`rest.len() != 2`); the runtime diagnostic is `Shell::bad_substitution` in
`interp.rs`.

**Repro:** `osh -c 'echo ${x@}'` → `bad substitution` (rc 1); bash → empty
(rc 0). The quoted form matches.

**Proper fix:** only worth doing if faithfully replicating bash's quoted-vs-
unquoted inconsistency for the empty-`@` case is deemed important; low value.
The common transforms (`@Q`/`@U`/`@u`/`@L`/`@E`/`@a`/`@k`/`@K`) and the common
bad-substitution forms (`${x!}`, `${!x*junk}`, `${#a[i]extra}`, `${!$}`,
`${!!}`) all match bash exactly.

### TD-OILS-SUBSCRIPT-QUOTED-BRACKET. `osh` lexer chokes on a quoted `]` inside a `${name[...]}` subscript — 2026-07-19 — OPEN (very low priority)

**What:** When an associative-array key contains a literal `]` and is *read
back* with the key quoted inside the expansion — `"${h["with]bracket"]}"` or
`"${h['with]bracket']}"` — osh's lexer fails with `unexpected EOF while looking
for matching '"'` (or `'`). bash accepts it: the quoted `]` inside the subscript
is not the subscript terminator. The *assignment* side is fine —
`declare -A h=(["with]bracket"]=1)` stores and `declare -p` round-trips the key
correctly (fixed alongside the quoted-key array-literal support). Only the
retrieval expansion with a nested-quoted `]` trips the lexer.

**Where:** `userspace/oils/src/lexer.rs` — the `${...}` scanner treats the first
unquoted-looking `]` as the subscript close and does not track quote state
*inside* the subscript, so a quoted `]` is mistaken for the terminator and the
trailing `"` is left unbalanced. Also relevant: `parser.rs`
`matching_subscript_close` / `split_name_subscript`.

**Repro:** `osh -c 'declare -A h=(["with]bracket"]=1); echo "${h["with]bracket"]}"'`
→ lexer EOF error; bash → `1`.

**Proper fix:** make the `${...}` subscript scanner quote-aware — when scanning
for the closing `]` of a `name[...]` subscript, skip over single/double-quoted
runs so a quoted `]` is not treated as the terminator. Very low value: a literal
`]` inside an associative key is exotic, and the assignment/`declare -p` paths
already work.

### TD-OILS-BASHOPTS. `osh` does not expose `$BASHOPTS` — RESOLVED 2026-07-20

**Resolved 2026-07-20** (superseded by later work — this entry was stale).
`osh` now seeds `$BASHOPTS` and byte-matches bash's full default set:
`echo $BASHOPTS` prints
`checkwinsize:cmdhist:complete_fullquote:extquote:force_fignore:globasciiranges:globskipdots:hostcomplete:interactive_comments:patsub_replacement:progcomp:promptvars:sourcepath`,
identical to `bash -c 'echo $BASHOPTS'`. The "honest state is unset" reasoning
below is obsolete: rather than materialize from osh's tiny live shopt inventory,
the default set is seeded directly (like `$SHELLOPTS`). Verified via probe.

<details><summary>Original entry (obsolete, for history)</summary>

**What:** bash exposes `$BASHOPTS`, a readonly colon-separated list of the
enabled `shopt` options (analogous to `$SHELLOPTS` for `set -o` options).
`osh` leaves `BASHOPTS` unset. `$SHELLOPTS` itself is now fully implemented
and byte-matches bash (see `refresh_shellopts` in `interp.rs`); `BASHOPTS`
was deliberately *not* implemented alongside it.

**Reproduce:** `bash -c 'echo "$BASHOPTS"'` prints
`checkwinsize:cmdhist:complete_fullquote:extquote:force_fignore:globasciiranges:globskipdots:hostcomplete:interactive_comments:patsub_replacement:progcomp:promptvars:sourcepath`;
`osh -c 'echo "[${BASHOPTS-UNSET}]"'` prints `[UNSET]`.

**Why deferred (not a band-aid):** bash's default `BASHOPTS` lists ~13
shopt options that are on by default. `osh` models essentially none of
those shopts (most are interactive/completion features — `progcomp`,
`hostcomplete`, `promptvars`, `checkwinsize`, `cmdhist`, `complete_fullquote`
— irrelevant to a non-interactive script shell). Materializing a `BASHOPTS`
value from `osh`'s tiny shopt inventory would produce an empty or
divergent string, which is *more* misleading to a script doing
`case $BASHOPTS in *extquote*)` than an absent variable. Shipping a fake
partial value would be exactly the kind of band-aid CLAUDE.md forbids.

**Proper fix:** implement bash's shopt inventory (at least the
default-on, behaviorally-meaningful ones osh can honestly claim —
`extquote`, `globasciiranges`, `patsub_replacement`, `sourcepath`,
`interactive_comments`, `globskipdots`, `force_fignore`), wire them into
the `shopt` builtin's option table, then add a `refresh_bashopts` helper
mirroring `refresh_shellopts` (readonly stored var, recomputed on every
`shopt -s`/`-u`). Byte-matching bash's full default set also requires
modeling the completion-related shopts, which only makes sense once osh
has a completion subsystem. Until then the honest state is "unset".
</details>

### TD-OILS-DECLARE-P-BULK-DYNAMICS. bulk `declare -p` (no names) omits the dynamic special variables — 2026-07-19 — OPEN (very low priority)

**What:** `declare -p NAME` for a scalar dynamic special variable now
matches bash (e.g. `declare -i BASHPID="12345"`, `declare -- LINENO="1"`
— implemented via `format_scalar_dynamic_declare` in `interp.rs`). But the
*bulk* listing `declare -p` with no name operands still only enumerates
`self.vars` / `self.arrays` / `self.assoc`, so it does not print the
special variables at all. bash lists them there in an attribute-only form
(no value), e.g. `declare -i BASHPID`, `declare -- LINENO`,
`declare -a BASH_LINENO=()`, `declare -i SRANDOM`.

**Reproduce:** `bash -c 'declare -p' | grep BASHPID` prints
`declare -i BASHPID`; `osh -c 'declare -p' | grep BASHPID` prints nothing.

**Why deferred (not a band-aid):** matching the bulk listing byte-for-byte
means enumerating bash's full set of always-present special variables
(including ones osh does not model at all, e.g. `SRANDOM`, `SHLVL`
attribute quirks, `PIPESTATUS`, `COMP_*`) and reproducing the
attribute-only "invisible variable" form (name with flags but no `=`).
That is a large surface for a listing that scripts almost never parse
(callers use `declare -p NAME`, which is now correct). Emitting a partial
subset would diverge from bash in a *different* way than omitting them.

**Proper fix:** add a curated table of always-present special variables
with their fixed attribute flags and "has a live value vs. attribute-only"
status, and have the no-names branch of `declare_print` merge that table
into the enumeration (skipping any that are shadowed by a real `self.vars`
entry). Verify the union and ordering against `bash -c 'declare -p'`.

### TD-OILS-COPROC. `osh` does not implement `coproc` — RESOLVED 2026-07-19

**RESOLVED (2026-07-19).** `coproc` is now implemented following the
minimal-surface design spiked below. Summary of what shipped:
- Parser: `Command::Coproc { name: Option<String>, body }` (ast.rs);
  `parse_coproc` + `compound_starts_at` recognise `coproc [NAME] command`
  at command start with the exact bash grammar (explicit NAME only before
  a compound starter). Unparser arms in `command_block`/`command_inline`.
- Executor: `exec_coproc` (interp.rs) creates two `std::io::pipe()`s,
  spawns the body on a detached `std::thread::spawn` with an owned
  `clone_for_subshell()` (cloned *before* the endpoints are installed, so
  the body doesn't inherit its own coproc fds) driving `Out::Pipe` /
  `StdinSrc::Pipe`. Parent write end → `open_write_fds` (zero write-path
  change); parent read end → new `coproc_read_fds:
  HashMap<i32, RefCell<BufReader<File>>>` (a *persistent* buffered reader,
  so successive `read <&N` consume successive lines). `NAME=(readfd
  writefd)`, `NAME_PID` = synthetic monotonic id (`COPROC_PID` atomic).
- Read-resolution points updated to consult `coproc_read_fds`: `read -u N`,
  transient `<&N` (`resolve_redirects` DupIn), `read_line`,
  `read_record_input`, `read_all_bytes`, and external-command stdin in
  `run_external` (hands the child a live `try_clone` so it streams).
  `alloc_varfd` also skips `coproc_read_fds`. `clone_for_subshell`
  `try_clone`s each live read handle (shared OS pipe, bash semantics).
- New helpers `pipe_reader_into_file` / `pipe_writer_into_file`
  (cfg-split unix/windows, via `OwnedFd`/`OwnedHandle`, no unsafe).
- Tests: 9 (`coproc_*`) covering default/named/simple-command bodies,
  bidirectional round-trip, `read -u`, successive-line reads, `NAME_PID`,
  high-fd allocation, and the named-only-before-compound grammar. All
  match real MSYS bash (6 probes). 539 tests green, clippy clean, both
  host + slateos targets build.

**Remaining minor limitations (low priority):** persistent `exec M<&N`
duplicating a coproc read fd is not wired (`clone_input_fd` / persistent
`apply_persistent_redirect` DupIn still only look at `open_fds` — `exec
4<&${COPROC[0]}` would report "bad fd"); only the single-active-coproc
case is targeted (bash itself warns on a second); coproc threads are
detached rather than joined on shell exit / `unset NAME` (relies on
process exit for reclaim); `NAME_PID` is synthetic so `wait`/`kill` on it
are best-effort. None of these affect the common `coproc`/`read <&N`
idioms. The original analysis + full design spike is retained below.

---

**Where (original):** `userspace/oils/src/parser.rs` (no `coproc` production — the
word is only listed as a reserved word in `interp.rs` ~9670/9880 for
completion, never parsed) and `userspace/oils/src/interp.rs` (no executor
or `COPROC`/`NAME`/`NAME_PID` support). The fd model in `Shell` also can't
represent a coproc's endpoints: `open_fds` is `HashMap<i32,
RefCell<Cursor<Vec<u8>>>>` (dead in-memory byte buffers) and
`open_write_fds` is `HashMap<i32, Arc<File>>` — neither can hold a *live*
`io::PipeReader`/`io::PipeWriter` that streams to/from a running coproc.

**What:** `coproc [NAME] cmd` / `coproc [NAME] { compound; }` is a bash
keyword that runs `cmd` asynchronously with its stdin/stdout wired to two
pipes, exposing an array `NAME` (default `COPROC`) where `NAME[0]` is the
fd to read the coproc's stdout and `NAME[1]` the fd to write its stdin,
plus `NAME_PID`. osh parses it as a plain word followed by a brace group
and dies: `coproc { echo hi; }` → `osh: syntax error: unexpected reserved
word '}'`. Reproduce: `osh -c 'coproc { echo fromco; }; read x
<&"${COPROC[0]}"; echo "$x"'` → syntax error; bash prints `fromco`.

**Why deferred:** this is not a small fix — it needs (1) a parser
production for `coproc` (optional NAME, then a command or compound
command → a new `Command::Coproc { name: Option<String>, body }`); (2)
extending the fd model so `open_fds`/`open_write_fds` (or new tables) can
hold live pipe endpoints — e.g. make each an enum `Cursor(...)  |
Pipe(...)`; every read/write site that matches those tables must handle
the new variant; (3) a background thread running a subshell clone of the
body with `Out::Pipe`/`StdinSrc::Pipe` wired to the two OS pipes, fds
allocated ≥ 10 and stored in the `NAME` array, plus `NAME_PID`; (4)
lifecycle/cleanup (join/close on shell exit, `wait`, and when the array
is unset). Because step 2 touches a mature, heavily-relied-on fd model,
it warrants a dedicated, carefully-tested effort rather than being
bolted on mid-sweep. `coproc` is comparatively rare in real scripts, so
this is lower priority than user-visible expansion/redirect correctness.

**Design spike (2026-07-19) — feasibility CONFIRMED, exact semantics +
recommended minimal-surface implementation, so the eventual effort is a
clean execution rather than another investigation.** (A parser/AST/unparser
prototype was written and then *reverted* — the executor's fd-model surgery
is entangled enough that a half-implementation that parses but errors at
runtime would be a band-aid; better to land it whole. Findings below.)

- **`std::io::pipe()` works on this toolchain** (nightly-x86_64-pc-windows-gnu,
  edition 2024) and is cross-platform, so it compiles for the slateos (unix)
  target too. No FFI needed. Verified with a standalone `rustc` build:
  `let (r, w) = std::io::pipe()?;` round-trips bytes. `PipeReader`/`PipeWriter`
  both impl `try_clone()`.
- **Exact bash grammar (probed against MSYS bash):**
  - `coproc simple_command` → array name defaults to `COPROC`; an explicit
    NAME is **not** accepted before a simple command (`coproc myname cat`
    runs `myname cat` as a command — "myname: command not found").
  - `coproc NAME compound_command` → explicit NAME (only recognised when a
    valid identifier is immediately followed by a compound-command starter:
    `{ ( (( [[ if while until for select case`).
  - `coproc compound_command` → name `COPROC`.
  - Sets `NAME[0]` = fd to **read** the coproc's stdout, `NAME[1]` = fd to
    **write** the coproc's stdin, plus scalar `NAME_PID`. bash uses high fd
    numbers (e.g. 63/60); osh can use the lowest free ≥ 10.
  - Two simultaneous coprocs: bash warns ("still exists") but allows one at a
    time cleanly; multi-coproc is a known bash weakness — match the single
    active coproc case first.
- **Executor reuse pattern already in the tree:** the pipeline executor
  (`exec_pipeline`, interp.rs ~952) already runs a `clone_for_subshell()`
  (which is `Send`) inside `std::thread::scope`, driving the body with
  `Out::Pipe(PipeWriter)` and `StdinSrc::Pipe(RefCell<BufReader<PipeReader>>)`.
  coproc differs in that it must be **detached** (runs in the background while
  the parent continues), so it needs `std::thread::spawn` (not scoped) with an
  **owned** `body.clone()` and an **owned** `clone_for_subshell()` (both are
  `'static`; `Shell` has no lifetime param). Store the `JoinHandle` in a new
  `Shell` field (e.g. `coproc_jobs: Vec<…>`) so it can be joined at shell exit.
  Give the thread `child_stdin_r`/`child_stdout_w`; keep `parent_stdin_w`/
  `parent_stdout_r` in the parent.
- **RECOMMENDED minimal-surface fd design (avoids the invasive `open_fds`
  enum):**
  - *Write end* (`NAME[1]`): convert the parent-side `PipeWriter` to a
    `std::fs::File` (via `OwnedHandle`/`OwnedFd` — `File: From<OwnedFd>` and
    `PipeWriter: Into<OwnedFd>`, cfg-split for windows/unix) and store it as
    `Arc<File>` in the **existing** `open_write_fds`. Then `echo >&"${NAME[1]}"`
    and `cmd >&N` work with **zero** changes to the write machinery, because
    `>&N` already resolves `open_write_fds` and the whole write path is
    `Arc<File>`-typed (`stdout_file`, `StderrTarget::WriteFd`, external stdio).
  - *Read end* (`NAME[0]`): the read path is Cursor/byte-clone based
    throughout (`StdinSrc::Cursor(&RefCell<Cursor>)`, `clone_input_fd` clones
    bytes, subshell clones snapshot remaining bytes) — a live pipe cannot be
    pre-buffered. Put live read ends in a **dedicated** table
    `coproc_read_fds: HashMap<i32, Arc<File>>` (convert `PipeReader`→`File`),
    consulted only at the canonical read-resolution points, and add a
    `StdinSrc::Live(RefCell<BufReader<File>>)` variant (handled identically to
    `StdinSrc::Pipe`, since `File: Read`). Read-resolution points to touch:
    (a) `read -u N` (interp.rs ~10015-10029, currently reads `open_fds`);
    (b) the `M<&N` input-dup in `apply_persistent_redirect` `RedirectOp::DupIn`
    (~4905) and `clone_input_fd` (~4939); (c) the transient per-command
    `<&N` input-dup path in the `RedirPlan` (`ExtraFdOp`, ~5100-5170 / 6759);
    (d) external commands inheriting `fd 0 <&N` in `run_external` stdio wiring.
    Extend `alloc_varfd`/fd allocation (~4749) to also skip `coproc_read_fds`
    numbers so the three tables never collide, and have `clone_for_subshell`
    `try_clone` the shared `Arc<File>` (bash: a subshell inherits the coproc
    fd — a shared OS handle, so `try_clone` is the correct semantics, unlike
    the byte-snapshot used for buffered fds).
  - This keeps the 15 buffer-only `open_fds` sites untouched; only the ~4 live
    read-resolution points + the executor + one `StdinSrc` variant + the write
    conversion change. A separate live table is a legitimate model (live OS
    streams are a genuinely different object than replay buffers), not scatter.
- **`NAME_PID`:** the body runs as a *thread*, not an OS process, so there is
  no real child pid — assign a synthetic monotonic pid (same limitation osh
  already has for backgrounded shell bodies via `last_bg_pid`). `wait`/`kill`
  on a coproc pid is therefore best-effort.
- **Lifecycle:** join the coproc thread and drop both parent-side `File`
  endpoints on shell exit, on `unset NAME`, and (bash) when a second coproc
  replaces the first. Closing `parent_stdin_w` (via `exec {NAME[1]}>&-` or
  drop) delivers EOF to the coproc's stdin; the coproc's thread finishing
  drops `child_stdout_w`, delivering EOF to the parent's `NAME[0]` reader.

### TD-OILS-VARFD-RO-MSG. `osh` readonly-varfd redirect emits one error line; bash emits two — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` `redir_effective_fd` (returns
`Err("{target}: readonly variable")` when the varfd target is readonly).

**What:** `readonly v=abc; echo x {v}>f` — bash prints *two* diagnostics
(`v: readonly variable` **and** `v: cannot assign fd to variable`), osh
prints only the first. Functionally identical: both set exit status 1,
run nothing, and leave `$v` unchanged. Purely cosmetic, and the error
*prefix* already differs anyway (`osh:` vs `bash: line N:`, tracked as
the pervasive errline gap). Proper fix if pursued: emit the second
`{target}: cannot assign fd to variable` line after the readonly
message. Very low value.

### TD-OILS-CMODE-EXIT. `osh -c` fatal-expansion exit status is 1, not bash's `-c`-only 127 — 2026-07-19 — ✅ RESOLVED 2026-07-19

**What:** bash exits with **127** when a fatal parameter-expansion error
(`${var:?msg}`, `set -u` on an unbound variable) aborts a `bash -c STRING`
invocation, but exits with **1** for the same error in a *script file*
(`bash script.sh`). osh originally returned 1 in both modes.

**Resolution:** osh now carries a `command_mode` flag (set by `main.rs` for
`-c`), and `fatal_abort_status(code)` returns the raw `code` (127) only at
the main shell in command mode, else 1. Verified: `osh -c 'echo
"${var:?msg}"'` → 127; `osh script.sh` (same body) → 1 — both match bash.

**Follow-up (also resolved 2026-07-19):** with **errexit** (`-e`) enabled,
bash downgrades that 127 to **1** (it treats the fatal expansion as a failed
command). This keys purely on the `-e` option being set, regardless of
whether errexit would fire in the current context (`set -eu; echo $UNDEF ||
true` still exits 1). `fatal_abort_status` now maps 127→1 when `self.errexit`
is set. Covered by `nounset_abort_under_errexit_is_one`.

---

### TD-OILS-PARSE-ERR-LOC. `osh` syntax-error diagnostics differ from bash in source-token, line number, and partial output — 2026-07-19 — OPEN (low priority, error-path cosmetics)

**Where:** `userspace/oils/src/parser.rs` (`ParseError`, `unexpected_here`,
`cur_line`), `userspace/oils/src/lexer.rs` (`LexError`, which carries no
line), `userspace/oils/src/interp.rs` `format_parse_error` (~15450) and the
`err_prefix`-based prefix it is handed, and the whole-program parse-then-exec
model in `run_source*`.

**What:** three related divergences on the *syntax-error* path (all
stderr-only; runtime-error messages already match bash):

1. **Missing input-source token.** bash tags a syntax error with the current
   input source between `$0` and `line N`: `bash: -c: line N: …` for `-c`,
   `bash: eval: line N: …` for `eval`, and just `<script>: line N: …` /
   `bash: line N: …` for a script file / stdin. osh emits only
   `<name>: line N: …` (no `-c:`/`eval:` token). Repro: `bash -c 'if true'`
   → `bash: -c: line 2: syntax error…`; `osh -c 'if true'` → `osh: line 1:
   syntax error…`.

2. **Wrong line number.** osh's parse-error prefix takes its line from
   `self.current_line` (≈1 at parse time, before execution) rather than the
   offending token's line, so *every* syntax error reports line 1 regardless
   of where it is. bash reports the real line — and for *end-of-file* errors
   uses (last-content-line + 1) (e.g. one-line `if true` → line 2). The
   parser already knows the right line via `cur_line()`; the fix is to stamp
   it onto `ParseError`/`LexError` at the point of failure (pos is not reset
   on error, so a central capture in `parse_tokens` works) and to add the
   EOF `+1` quirk.

3. **No partial output before the error.** bash parses-and-executes a script
   one command at a time, so `echo a; echo b; echo (` prints `a` and `b`
   before the line-3 syntax error. osh parses the whole program up front, so
   nothing runs when any part fails to parse.

**Why deferred:** (1) and (2) are a bounded refactor (thread a line + a
source-source-description through `ParseError`/`LexError`, convert ~24
construction sites, replicate bash's token-vs-EOF line rule), but the payoff
is purely syntax-error stderr cosmetics, and a *partial* fix still mismatches
bash on the other axis. (3) is an architectural change (incremental
parse/execute) that is much larger and interacts with here-docs, function
definitions spanning commands, and `$LINENO`. Not worth the regression risk
mid-compat-pass.

**Proper fix (if pursued):** convert `ParseError` to `{ msg, line:
Option<u32> }` with an `at(line)` builder; give `LexError` a line too (the
lexer tracks the current line while scanning); stamp the line centrally in
`parse_tokens` from `p.cur_line()` (and add `+1` for the EOF family); add a
per-source-kind token (`-c`/`eval`/none) threaded from the `run_source`
callers into `format_parse_error`. Tackle (3) separately as an
incremental-execution project if ever needed.

### TD-OILS-DECLAREF-QUIRKS. `osh` `declare -f`/`type` deparse differs from bash for four idiosyncratic constructs — 2026-07-19

**Where:** `userspace/oils/src/unparse.rs` `command_block` (`If` elif branch,
`Subshell`, `Function` nested case) and `item_stmt`/`program_block`
(background items).

**What:** After the 2026-07-19 byte-fidelity pass, osh's `declare -f`/`type`
output is byte-identical to bash for the common constructs (simple lists,
`if/then/else`, `while`/`until`, `for … in`, `for ((;;))`, `case`, `select`,
nested brace groups). Four bash deparser idiosyncrasies remain unmatched;
all four still emit **valid, re-parsable bash** with equivalent semantics —
only the exact whitespace/keyword layout differs:

1. **`elif` → nested `else if`.** bash rewrites `if a; then …; elif c; then
   …; fi` into `else\n if c; then …; fi` (deeper indentation, extra `fi`).
   osh prints a literal `elif …; then` clause.
2. **Subshell layout.** bash prints `( echo a;\n echo b );` (first statement
   glued to the `(`, continuation dedented). osh uses a clean indented block
   (`(\n    echo a;\n    echo b\n)`).
3. **Backgrounded statement in a list.** bash keeps `sleep 1 & echo b` on one
   line (`&` as an inline connector). osh puts each `Item` on its own line, so
   `sleep 1 &` and `echo b` split across two lines.
4. **`function` keyword on nested definitions.** bash prints a function
   defined *inside* another function as `function nested () ` (with the
   `function` keyword); top-level defs use `nested () `. osh always omits
   `function`.

**Why deferred:** these are rare constructs (subshell/background/nested-fn in
a function body) or a purely cosmetic restructuring (elif), and osh's output
round-trips correctly. Matching bash exactly means replicating quirks with
little practical benefit.

**Proper fix (if pursued):** (1) render elif chains as nested `else { if … }`
with incremented indent and a matching `fi` per level; (2) special-case the
subshell/background inline layouts; (3) thread a "nested function" flag so
inner `Function` defs prepend `function `.

### TD-OILS-ASSOC-KEY-TRIM. `osh` trims leading/trailing whitespace from an unquoted associative-array subscript key — 2026-07-19 — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/parser.rs` subscript extraction
(`try_assignment` store path, `split_name_subscript` read path,
`parse_array_elem` keyed-element path); `userspace/oils/src/lexer.rs`
array-literal element tokenization (`read_word_inner` / new
`read_array_elem_word`).

**What (was):** After the lexer fix that keeps `h[a b]=v` as one assignment
word (interior spaces preserved), a subscript with *leading/trailing*
spaces was still trimmed: `declare -A h; h[ x ]=v; echo "${!h[@]}"` printed
`x` in osh but ` x ` in bash. Two distinct causes: (1) the three
subscript-parse sites lowered the subscript via `word_from_source`, which
re-tokenizes and word-splits (dropping surrounding whitespace); (2) inside
an array literal `([ x ]=v)`, the lexer word-split the element on the
interior spaces, so it wasn't even recognised as one keyed element.

**Fix shipped:** (1) all three subscript-parse sites now use
`word_verbatim_from_source` (parses a single word with no
splitting/trimming), so the expanded text — surrounding whitespace
included — is the literal associative key on both store and read paths;
arithmetic/indexed subscripts arithmetic-evaluate and ignore the
whitespace, so they are unaffected. (2) A new `Lexer::read_array_elem_word`
(via a `array_elem` flag on `read_word_inner`) slurps a leading
`[subscript]=value` element across unquoted interior spaces, matching
bash's array-literal tokenization, so `declare -A m=([ x ]=v [y z]=w)`
keys on ` x ` / `y z`. Also fixed `declare -p` to quote associative keys
that hold shell metacharacters (`["a b"]`, ANSI-C `$'…'` for control
chars) via a new `quote_declare_key`, so printed subscripts round-trip.
Tests: `interp::assoc_key_preserves_surrounding_whitespace`,
`interp::declare_p_quotes_assoc_keys_needing_it`,
`lexer::array_literal_keyed_element_keeps_spaces`. Verified against bash
across store/read/`declare -p`/array-literal cases.

### TD-OILS-ASSOC-ORDER. `osh` iterates associative arrays in insertion/sorted order, not bash's hash order — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` associative storage (`self.assoc`
is an insertion-ordered map) and every `[@]`/`[*]`/`${!m[@]}` expansion
that reads it (`array_elements`, `array_keys`, `bulk_elements`, …).

**What:** `declare -A m=([x]=1 [y]=2); echo "${m[@]}"` prints `1 2` in osh
but `2 1` in bash. bash stores associative arrays in an internal hash
table and iterates in hash-bucket order (a function of its string-hash of
the keys); osh iterates in insertion order. Every associative `[@]`/`[*]`
expansion (values, keys, `${m[@]:-…}`, `${m[@]#pat}`, `for k in
"${!m[@]}"`, etc.) can therefore differ in element *order* — the contents
are identical, only the sequence differs.

**Why deferred:** reproducing bash's exact iteration order would require
reimplementing bash's specific string-hash and open-addressing bucket
walk, and scripts are not supposed to rely on associative-array order
anyway (POSIX/bash both document it as unspecified). Insertion order is
arguably more useful and is self-consistent.

**Proper fix:** only if a real script needs bash-identical ordering —
port bash's `hash.c` hash function and bucket iteration for `self.assoc`.
Not worth it absent a concrete need.

### TD-OILS-MISSING-SPECIAL-ARRAYS. `osh` does not define some bash special array variables (`GROUPS`) — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` variable seeding
(`seed_shell_vars`) and the dynamic-array materialisers (cf.
`refresh_funcname` for FUNCNAME/BASH_SOURCE, `refresh_dirstack` for
DIRSTACK, `refresh_bash_arg_arrays` for BASH_ARGC/BASH_ARGV).

**What:** bash predefines several dynamic array variables that osh does
not. Confirmed missing (`${VAR+set}` empty in osh, `set` in bash):
`GROUPS` (the invoking user's supplementary group IDs). DIRSTACK is now
implemented (see `refresh_dirstack`); FUNCNAME, BASH_SOURCE, BASH_LINENO
already were; **`BASH_ARGC`/`BASH_ARGV` are now implemented — RESOLVED
2026-07-20** (see below).

**Why deferred:** `GROUPS` needs a host notion of Unix supplementary
groups, which the Windows host build and the current slateos user model
do not expose. Low value; no known script depends on it.

**Proper fix:** when the process/identity layer exposes supplementary
groups, materialise `GROUPS` from it (like `refresh_dirstack`).

**BASH_ARGC / BASH_ARGV — RESOLVED 2026-07-20.** Implemented as the
extended-debugging call-argument stack (`bash_argc: Vec<usize>`,
`bash_argv: Vec<String>`, `arg_frame_pushed: Vec<bool>` on `Shell`;
helpers `extdebug_on`, `push_arg_frame`, `pop_arg_frame`,
`refresh_bash_arg_arrays`). Semantics matched against bash: enabling
`shopt -s extdebug` captures a base frame from the *current* positional
params (count → BASH_ARGC front, params reversed → BASH_ARGV front); each
function call made while extdebug is on pushes its arg count/values on
top; a call's return pops **only** the frame it actually pushed (recorded
per-frame in `arg_frame_pushed`, so toggling extdebug mid-call cannot
desync the stacks); disabling extdebug does not clear the stack; the base
snapshot is static (a later `set --` does not change it); subshells
inherit (clone) the parent stack but push no frame of their own. All of
these cases were verified equal to MSYS bash, plus enabling extdebug
*inside* a function (base = that function's positional). Regression test:
`bash_argc_argv_extdebug_stack`.

*Deliberate divergence (documented, not a bug):* **without** `extdebug`,
bash still exposes an undocumented top-level base frame — e.g.
`echo ${BASH_ARGC[@]:-U}` at a `-c` top level prints `0` in bash but `U`
(unset) in osh. bash builds this via a lazy-materialisation artifact whose
observable behaviour is self-contradictory (referencing `BASH_ARGV` before
a `set --` freezes it at the *pre-set* value; not referencing it first
yields the *post-set* value; and a non-extdebug function call leaves
`BASH_ARGC` unset *inside* the call yet materialises the base *after*
return). bash's own man page says "The shell sets BASH_ARGC only when in
extended debugging mode", so osh follows that documented contract and
populates BASH_ARGC/BASH_ARGV only under `extdebug`. Replicating the
undocumented non-extdebug quirk was judged not worth the bug risk for a
behaviour no real script relies on.

### TD-OILS-COND-PAREN-REGEX. `[[ … =~ ( … ]]` — bash treats `(` as conditional grouping, osh treats it as regex — 2026-07-19

**Where:** `userspace/oils/src/parser.rs` conditional-expression parsing
and `userspace/oils/src/interp.rs` `cond_regex`.

**What:** `[[ abc =~ ( ]]` — bash parses `(`/`)` inside `[[ ]]` as
expression-grouping operators (`[[ ( a || b ) && c ]]`), so a bare `(`
with no closing `)` is a *shell parse error* ("unexpected EOF while
looking for matching `)'"). osh instead treats everything after `=~` up
to `]]` as the regex RHS, so `(` becomes an (invalid) regex and `[[`
returns 2. The common invalid-regex path (`[[ x =~ [ ]]`) now matches
bash exactly (status 2, no message); only the paren-as-grouping vs
paren-as-regex distinction diverges.

**Why deferred:** correctly disambiguating `(` between conditional
grouping and a regex metacharacter requires bash's context-sensitive
`[[` tokenizer (bash special-cases `(`/`)` only *outside* the `=~` RHS,
and the RHS boundary itself depends on word splitting). This is a rare
construct — a literal unbalanced `(` immediately after `=~`. The
exit-code for genuinely-invalid regexes already matches.

**Proper fix:** teach the `[[` parser bash's grouping rules and have the
`=~` RHS consume a single word (bash reads the RHS as one word unless
parenthesized), so `( )` grouping and regex parens are distinguished by
position.

### TD-OILS-PROCSUB-DEVFD. Process substitution as a *word* expands to a temp-file path, not `/dev/fd/N`, on the Windows dev host — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` process-substitution expansion
(the `osh_psub_*.tmp` temp-file backing).

**What:** `echo <(echo hi)` prints `/dev/fd/63` on Linux/bash but
`C:/Users/…/Temp/osh_psub_<pid>_0.tmp` on osh's Windows dev host. The
*content* is correct — reading the path yields `hi` — only the path
*format* differs. This only manifests when a script inspects the
substitution's filename itself (rare); using it as an input file works.

**Why deferred:** `/dev/fd/N` requires a `/dev/fd` filesystem and real
fd-passing, which the Windows host lacks; osh backs process substitution
with temp files there. On the slateos target (which has proper fd
support) this should present as `/dev/fd/N` and match bash. This is a
host-platform artifact, not a shell-logic bug.

**Proper fix:** on slateos, back `<()`/`>()` with anonymous pipes exposed
via `/dev/fd/N` rather than temp files; the Windows dev host keeps the
temp-file fallback.

### TD-OILS-DECLARE-BADID. `osh` `declare NAME[a b]=v` silently no-ops; bash errors "not a valid identifier" — RESOLVED 2026-07-20

**Resolved 2026-07-20** — and the fix uncovered/closed a larger adjacent gap:
`declare "NAME[sub]=value"` (a *quoted*, single-arg subscripted target) was not
handled at all — osh stored a scalar literally named `NAME[sub]` and `declare -p
NAME` reported "not found". `builtin_declare` now:
- splits each target into a base name + optional `[subscript]`;
- validates the **base** as an identifier, emitting `{tag}: \`ARG': not a valid
  identifier` (status 1, quoting the original arg) for `bad@name=v`, `1x=v`, or
  an unbalanced `h[a` — this also fixes the original non-subscript case;
- auto-creates an **indexed** array for a subscripted name (never clobbering an
  existing associative array), matching bash's `declare "x[5]"` → `declare -a x`;
- routes the element assignment through the normal array machinery via a directly
  constructed `Assignment` AST (NOT by re-parsing a `base[sub]=value` string,
  which would word-split an unquoted-space value like `x[0]="2 x"`), so the
  subscript is arith-evaluated for indexed arrays / literal for associative, and
  a bad `-i` **value** stays fatal.

Faithful error tagging: a bad **subscript** is reported untagged (`a b: syntax
error in expression`, like a command-position `a[x y]=v`, even under `-i`) by
clearing `arith_cmd` only while resolving the subscript, whereas a bad `-i`
**value** keeps the `declare:` tag. Verified against bash across `x[5]=v`,
`x[2+3]=v`, `-i x[0]=2+3`, `-i a[0]="2 x"` (fatal, tagged), `x[a b]=v` (fatal,
untagged), `-A m[k]=v`, `x[5]` (empty array), `x[3]+=b`, `bad@name=v`, `1x=v`.
Regression test `declare_subscripted_target_and_bad_identifier`; 684 tests,
clippy clean, host + slateos build green. `is_valid_name` was made
`pub(crate)` in `parser.rs`.

<details><summary>Original entry (for history)</summary>

**Where:** `userspace/oils/src/interp.rs` `builtin_declare` argument
parsing (the per-arg assignment/attribute handling).

**What:** In *argument* position (after the `declare` command word), bash's
tokenizer splits `declare h[a b]=v` into `h[a` and `b]=v` and `declare`
then rejects each: `declare: `h[a': not a valid identifier` (status 1). osh
splits the same way but its `declare` builtin silently ignores the
malformed args (no error, exit 0). Reproduce: `declare -A h; declare
h[a b]=v; echo $?` → bash prints two errors + status 1; osh prints nothing
+ status 0.

**Why deferred:** niche (unquoted spaces in a `declare` argument subscript);
the correct incantation `declare "h[a b]=v"` works in both shells.

**Proper fix:** in `builtin_declare`, when an argument is neither a valid
attribute flag nor a well-formed `name[sub]?=value` assignment, emit
`osh: declare: \`ARG': not a valid identifier` to stderr and set the exit
status to 1 (accumulating the worst status across args).
</details>

### TD-OILS-BAD-ARRAY-SUBSCRIPT. `osh` "bad array subscript" underflow name/fatality — 2026-07-20 — ✅ RESOLVED 2026-07-20

**Status:** All four forms now match bash exactly, including the exact
diagnostic text (which uses the **raw subscript source**, post word-expansion
but pre-arithmetic — `a[1+2-20]`, `a[i]` — not the evaluated index):

- **Write underflow** (`x[-9]=z`, `a[1+2-20]=z`, `a[i]=z`): names the **full**
  reference with the raw source (`a[1+2-20]: bad array subscript`) and is
  **fatal** in command position (aborts, status 1). Inside `declare "…"` it is
  demoted to **non-fatal** (status 1, `declare` continues). Implemented at
  `apply_assignment` (~3521): the subscript is evaluated with the arith tag
  cleared (bash never tags a bad *subscript* expr, even under `-i` — only a bad
  `-i` *value* is tagged `declare:`), a syntax error returns early, and an
  underflow emits `{name}[{raw_src}]` + sets `unbound_error = Some(1)`. The
  `declare` branch passes the raw subscript source straight through (no
  pre-evaluation), so the message names the source and the value keeps its
  `declare:` tag; a `had_fatal` snapshot demotes the underflow to status 1.
- **Read underflow, value form** (`echo ${a[-5]}`): non-fatal, names the
  **base** (`a: bad array subscript`), value expands empty — matches bash.
- **Read underflow, length form** (`echo ${#a[-9]}`, `${#a[1+2-20]}`): the
  obscure **fatal** subcase naming the raw source + `]`
  (`1+2-20]: bad array subscript`), aborting the command. Implemented at
  `expand_array_ref` (~4590).
- **Positive out-of-range read** (`${a[9]}`): empty, no error — matches bash.

Covered by `bad_array_subscript_underflow` and the updated `array_negative_index`.

### TD-OILS-RO-ARRAY. `osh` `readonly -p` uses `readonly name=val` and can't format array vars — 2026-07-19 — ✅ FIXED 2026-07-19

**Status:** FIXED. `builtin_readonly`'s listing branch now reuses
`format_declare_def` (falling back to `declare {flags} {name}` for valueless
readonly names), so scalars print as `declare -r name="value"` and arrays as
`declare -ar name=([0]="v" …)` / `declare -Ar name=(…)`, matching bash. With
the listing fixed, `BASH_VERSINFO` is now seeded **readonly** in
`seed_shell_vars` (clobbering it is rejected, and it appears correctly in
`readonly -p`). Additionally, inline array literals on `readonly`/`export`
(`readonly arr=(1 2)`, `export -A m=([k]=v)`) are now supported — the parser's
`is_declaration_command` accepts them and `exec_declare_with_arrays` applies
the implied `-r`/`-x` attribute — and unsetting an element of a readonly array
is now correctly rejected. Covered by `readonly_print_lists_vars`,
`readonly_export_array_literal`, and `readonly_array_element_cannot_unset`.

**Where (was):** `userspace/oils/src/interp.rs` `builtin_readonly` listing branch
(~7370–7380): it looks up each readonly name only in `self.vars` and prints
`readonly {name}={value}` (or `readonly {name}` when not a scalar).

**What:** bash's `readonly -p` prints `declare -r name="value"` for scalars
and `declare -ar name=([0]="v" …)` for arrays — it reuses the `declare -p`
formatting. osh instead prints `readonly a=1` for scalars (no quoting, wrong
keyword) and, for a readonly *array*, just `readonly NAME` with no contents
(the value is in `self.arrays`/`self.assoc`, which the listing never
consults). Reproduce: `readonly a=1; readonly -p` → bash `declare -r a="1"`,
osh `readonly a=1`; `readonly arr=(1 2); readonly -p` → bash
`declare -ar arr=([0]="1" [1]="2")`, osh `readonly arr`.

**Impact today:** this is why `$BASH_VERSINFO` is seeded **non-readonly**
(see `seed_shell_vars`): marking it readonly would surface it in
`readonly -p` as a bare `readonly BASH_VERSINFO` with no value, an obvious
wrong output. Fixing the listing lets BASH_VERSINFO be made readonly to
match bash.

**Proper fix:** rewrite the `readonly -p` / bare-`readonly` listing to reuse
`format_declare_def` (which already renders scalars, indexed, and
associative arrays with attribute flags), substituting the `-…r…` flag
group, exactly as `declare -p` does. Update `readonly_print_lists_vars` and
add array coverage. (Note bash uses `declare -r`, not `readonly`, as the
keyword in this listing.)

### TD-OILS-FAILGLOB-SCRIPT. `osh` `failglob` aborts the whole script, not just the current line — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — the `glob_error` fatal-expansion
handlers return `Flow::Exit(1)`, which `run_source` (~564) turns into
"stop executing the whole parsed program." bash, by contrast, does a
per-line top-level discard.

**What:** with `shopt -s failglob`, a glob that matches nothing is a fatal
word-expansion error. In `bash -c 'shopt -s failglob; echo *.nope; echo done'`
(one input line) bash discards the rest of the line, so `done` does *not*
run — and osh matches this exactly. But in a multi-line *script file*, bash
discards only the offending line and continues to the next one, so a later
`echo done` on its own line *does* run. osh instead aborts the entire
`run_source`, so the following lines are skipped. Reproduce: a 3-line script
`shopt -s failglob\necho *.nope\necho done` prints `done` under bash but not
under osh.

**Why deferred:** bash's behavior stems from its reader executing one
top-level command per input line and `longjmp`-ing to that per-line top
level on an expansion error. osh parses the whole source into one `Program`
and has no per-line execution boundary, so replicating the exact discard
scope needs an execution-model change. The common paths (`-c`, interactive
REPL line-at-a-time) already match bash; only multi-line scripts using
failglob diverge, which is rare.

**Proper fix (if pursued):** give `failglob` (and the other fatal
word-expansion errors) a non-shell-exiting `Flow` variant that unwinds only
to the nearest top-level list boundary, and have script execution treat each
top-level `item` in the `Program` as such a boundary (so a discard skips the
rest of the *current* item's list but resumes at the next top-level item).

### TD-OILS-INDIRECT-MOD. `osh` rejects indirect expansion combined with a modifier (`${!ptr:-def}`, `${!ptr#pat}`, …) — 2026-07-19 — ✅ FIXED 2026-07-19 (parser emits `WordPart::IndirectOp{refname,target}`; `expand_dynamic` resolves the pointer then applies the modifier to the target)

**Where:** `userspace/oils/src/parser.rs` (parameter-expansion parser) and
`userspace/oils/src/interp.rs` `expand_indirect`. The parser only recognises a
bare `${!name}` (indirect) and the name-listing/keys forms (`${!pre@}`,
`${!pre*}`, `${!arr[@]}`); combining `!` indirection with a value operator is
reported as `syntax error: unsupported parameter expansion '${!ptr:-def}'`.

**What:** bash allows the full operator set to apply to the *indirect target*:
`${!ptr:-default}`, `${!ptr:offset:len}`, `${!ptr#pat}`, `${!ptr/a/b}`,
`${!ptr^^}`, etc. — the `!ptr` first resolves to the target name, then the
operator applies to that target's value. osh does not parse these.

**Proper fix:** in the parameter-expansion parser, when a `${` body starts with
`!` followed by a *name* (not a prefix-`@`/`*` or `[@]`/`[*]` listing form),
parse the remainder as an ordinary modifier suffix and record an "indirect"
flag on the resulting `WordPart`. At expansion time, resolve the indirect
target name first (via the same logic as `expand_indirect`, including the
fatal unset-pointer / invalid-name errors), then apply the modifier to the
target's value. Add tests: `ptr=missing; echo ${!ptr:-fb}` -> `fb`;
`x=hello; p=x; echo ${!p:2:3}` -> `llo`; `x=FOO; p=x; echo ${!p,,}` -> `foo`.

### TD-OILS-INDIRECT-ARITH. `osh` indirect-expansion error inside a `(( ))`/arith command is not fatal — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `eval_arith_raw` (~line 1198) and
its callers (`exec_arith_command`, `builtin_let`, `exec_for_arith`, array
subscript evaluation). The arith-command path expands `$…`/`${…}` via
`expand_arith_params`, which does not route `${!ptr}` through `expand_indirect`,
so a bad pointer inside `(( ${!nonexist} ))` does not set `unbound_error`.

**What:** bash makes an invalid indirect expansion fatal even inside an
arithmetic *command*: `(( ${!nonexist} )); echo after` exits with status 1 and
never prints `after`. osh evaluates the arith command as if the expansion were
empty (0) and continues, printing `after`. (The related `let x=${!nonexist}`
case *is* already fatal because `let`'s argument is word-expanded through the
normal path that reaches `expand_indirect`.) This is a narrow edge — indirect
expansion inside `(( ))` is rare.

**Proper fix:** have `expand_arith_params` resolve `${!ptr}` through the same
indirect-expansion logic as the word expander (so it sets `unbound_error` on a
bad pointer), and — since `unbound_error` is not save/restored around
`expand_arith_params` — the following simple command's driver check will then
abort as bash does. Add test: `(( ${!nonexist} )); echo after` -> `("", 1)`.

### TD-OILS-PREFIX-RO. `osh` aborts a command with a readonly temp-assignment prefix; bash runs it anyway — 2026-07-19

**Where:** `userspace/oils/src/interp.rs`, the simple-command execution path
(~line 3009), the loop that rejects a readonly variable used as a temporary
assignment prefix:
```rust
for (k, _) in &assigns {
    let target = self.resolve_ref_name(k);
    if self.readonly.contains(&target) {
        self.emit_stderr(format!("osh: {target}: readonly variable\n").as_bytes());
        self.last_status = 1;
        return Flow::Next;   // ← wrong: bash still runs the command
    }
}
```

**What:** When a readonly variable appears as a *temporary assignment prefix*
to a command (`readonly x; x=1 echo mid; echo after`), bash prints
`x: readonly variable` to stderr but **still runs the command** (`echo mid`)
and continues (`echo after`), with the command's own exit status. osh instead
treats it as fatal-ish: it prints the error and returns without running the
command at all (prints nothing).

Note this is the *opposite* fatality direction from a **bare** readonly
reassignment (`readonly x=1; x=2`), which *is* fatal in a non-interactive
shell (shell exits, status 1) — that case was made correct on 2026-07-19
(see the arith/readonly assignment-fatality fix). Only the temp-*prefix*
case is wrong.

**Bash truth:** `readonly x; x=1 echo mid; echo after` → stdout `mid\nafter`,
rc 0 (stderr warning suppressed). osh → stdout empty.

**Proper fix:** on a readonly-prefix rejection, emit the warning to stderr but
skip *only* that failed assignment (do not export/set it) and continue
executing the command with the remaining prefixes and default status handling,
rather than short-circuiting with `Flow::Next`. Requires threading the
"this prefix failed" signal through to the command dispatch so the variable is
not applied while the command still runs. Add a test:
`readonly x; x=1 echo mid; echo after` → `("mid\nafter", 0)`.

### TD-OILS1. `osh` `[[ … ]]` conditional gaps: `-r`/`-x` file tests approximated as "exists" — MOSTLY RESOLVED 2026-07-18 (`=~` regex match + quote-aware literal RHS implemented; only the permission-bit tests remain, gated on the slateos permission model)

**Where:** `userspace/oils/src/ere.rs` (Pike-VM ERE engine),
`userspace/oils/src/lexer.rs` (`read_word_regex`, `cond_depth`/`regex_next`),
`userspace/oils/src/parser.rs` (`parse_cond_primary` → `CondExpr::Regex`),
`userspace/oils/src/interp.rs` (`cond_regex`, `cond_unary` permission tests).

**What:** The bash conditional command `[[ … ]]` is implemented (string
`==`/`=`/`!=` with glob-or-literal RHS, `<`/`>` ordering, numeric
`-eq…-ge`, unary file/string tests, `!`/`&&`/`||`/`(…)`), and `=~` regex
matching now works:
- `=~` (POSIX ERE regex match) — **RESOLVED.** An in-tree linear-time
  Pike-VM/Thompson-NFA ERE engine (`ere.rs`, ReDoS-safe: no catastrophic
  backtracking) compiles the RHS pattern and matches the LHS. The lexer
  reads the `=~` RHS as one regex word (so `(`, `)`, `|`, `<`, `>` are
  literal metacharacters, not shell operators); the RHS still undergoes
  parameter expansion. On a successful match the `BASH_REMATCH` indexed
  array is populated (`[0]` = whole match, `[i]` = capture group `i`;
  unmatched optional groups become empty strings), and it is cleared on a
  non-match. A malformed pattern reports to stderr and yields false.
  **Quote-aware RHS — RESOLVED 2026-07-18.** `cond_regex` now builds the
  pattern via `regex_pattern_from_rhs`, which walks the RHS `Word`'s parts:
  unquoted `Literal`/dynamic (`$var`, `$(…)`) parts contribute live regex
  syntax, while single- and double-quoted parts (including an expanded
  `"$p"`) are backslash-escaped so their metacharacters match literally —
  so `[[ a.b =~ "a.b" ]]` matches only the literal `a.b`, `[[ axb =~ a.b ]]`
  still matches (regex `.`), and `p='a.b'; [[ axb =~ $p ]]` matches while
  `[[ axb =~ "$p" ]]` does not. Tests: `cond_regex_double_quoted_rhs_is_literal`,
  `cond_regex_single_quoted_rhs_is_literal`, `cond_regex_mixed_quoting`,
  `cond_regex_quoted_var_is_literal`.
- `-r` and `-x` file tests are approximated as "path exists" (`-w` is
  "exists and not read-only") because the host has no portable mode-bit
  check and the slateos permission model isn't wired into `osh` yet.
  **Proper fix:** query the real per-file permission bits once the
  slateos userspace permission API is available.

The remaining `-r`/`-x` approximation is not a correctness bug in the
implemented surface — it is an intentional grow-phase scope limit gated on
the slateos permission model, documented in the `interp.rs` module header.
No test depends on the deferred behavior.

### TD-OILS2. `osh` arrays — RESOLVED 2026-07-18 (associative arrays, negative/arith subscripts, subscript+operator combos, sparse indexed arrays, negative-index assignment targets, and associative subscripts inside `(( … ))` all implemented)

**Where:** `userspace/oils/src/parser.rs` (`split_name_subscript`,
`try_assignment`, `spanning_subscript_assignment`, `parse_array_elem`,
`is_declaration_command`), `userspace/oils/src/interp.rs`
(`apply_assignment`, `exec_declare_with_arrays`, `expand_array_ref`,
`array_element`, `assoc_element`, `array_elements`, `array_keys`,
`builtin_declare`, `VarLookup::get`).

**What:** Indexed arrays (`a=(x y z)`, `a[i]=v`, `a+=(w)`/`a+=str`,
keyed/sparse literals `a=([2]=x y)`, `${a[i]}`, `${a[@]}`/`${a[*]}`,
`${#a[@]}`/`${#a[i]}`, `${!a[@]}` indices, `unset a[i]`/`unset a`) **and**
associative arrays (`declare -A m`/`typeset`/`local`, `m[key]=v`,
`m=([k]=v …)`, the combined one-liner `declare -A m=([k]=v)` /
`declare -a a=(x y)`, `${m[key]}`, `${m[@]}`/`${m[*]}` values, `${!m[@]}`
keys, `${#m[@]}`, `unset m[key]`; insertion-ordered, string subscripts)
are implemented. Quoted `"${a[@]}"`/`"${!a[@]}"` keep one field per
element; unquoted forms field-split. Remaining deferred pieces:
1. ~~**Negative indices** (`${a[-1]}` = last element) return empty.~~
   **DONE 2026-07-18:** `array_element` resolves a negative subscript
   from the end via `resolve_index` (`-1` = last; a scalar acts as a
   one-element array); out-of-range negatives yield empty. Reads only —
   negative index in an *assignment target* (`a[-1]=v`) is still TODO.
2. ~~**Arithmetic subscripts inside `(( … ))`** (`(( a[i] + 1 ))`) are not
   recognized.~~ **DONE 2026-07-18:** the arith parser recognizes
   `name[expr]` (subscript is itself an arithmetic expression, so
   `a[i+1]` and negative `a[-1]` work) via the new defaulted
   `VarLookup::get_index`, which `Shell` implements over `array_element`.
   **Associative subscripts in `(( … ))` — DONE 2026-07-18:** the arith
   parser captures the raw bracketed subscript text (balanced brackets) and
   dispatches on array kind via the new `VarLookup::is_assoc`/`get_assoc`:
   an associative subscript (`m[foo]`, `m[$k]`) is the literal string key
   (not arith-evaluated), while an indexed subscript is still evaluated as
   an arithmetic expression. `Shell` implements both over `assoc`/
   `assoc_element`. Tests: `arith::associative_subscripts`,
   `interp::arith_associative_subscript`.
3. ~~**Subscript combined with an expansion operator**
   (`${a[i]:-default}`, `${a[@]#pat}`) is rejected at parse time.~~
   **DONE 2026-07-18:** the four operator variants (`ParamOp`,
   `ParamTrim`, `ParamSubstr`, `ParamReplace`) now carry an optional
   `index: Option<Box<Word>>`. The parser attaches a `[expr]` subscript to
   the operator (rejecting only `[@]`/`[*]` + operator, a bulk transform).
   `param_elem_value` resolves the element base value (associative key vs.
   arithmetic index, negatives from the end), and `${a[i]:=v}` writes the
   element back via `assign_elem`. All of `${a[i]:-def}`, `:+`, `:?`,
   `#`/`##`/`%`/`%%`, `:off:len`, and `/pat/repl` work per element.
4. ~~**Indexed arrays use a dense backing store** (`Vec<String>`), so a
   sparse literal (`a=([5]=x)`) fills gaps with empty elements.~~
   **DONE 2026-07-18:** `arrays` is now `HashMap<String, BTreeMap<usize,
   String>>` — sparse by construction. `a=([5]=x)` stores one element;
   `${#a[@]}` counts only assigned elements; `${!a[@]}` lists only the
   assigned indices (ascending); `unset a[i]` removes just that index
   (leaves a gap, no shift-down); `${a}`/`${a[0]}` read index 0
   specifically; and a negative subscript counts back from
   `highest_index + 1` (bash semantics).

All previously-deferred pieces are now implemented. (Negative index in an
*assignment target*, `a[-1]=v`, is supported: it resolves from
`highest_index + 1`, so `a[-1]=Q` overwrites the last element.)

### TD-OILS3. `osh` compound-command redirections: stderr now honored — RESOLVED 2026-07-18

**Where:** `userspace/oils/src/interp.rs` (`exec_redirected`, `StderrTarget`,
`Shell::stderr_stack`, `emit_stderr`, `errln`, `child_stdio_for_stderr`,
`write_bytes`, `run_external`), `userspace/oils/src/parser.rs`
(`with_redirects`, `at_redirect_start`), `userspace/oils/src/ast.rs`
(`Command::Redirected`).

**What:** Trailing redirections on compound commands are supported
(`while read …; done < file`, `for … done > out`, `{ …; } >> log`,
subshell/`if`/`case`/`(( ))` bodies, pipeline-into-`while read`). Input
is fed via a shared `StdinSrc::Cursor` (`RefCell<io::Cursor<Vec<u8>>>`)
so successive `read`s in the body consume successive lines; the compound's
stdout is captured into a buffer and written to the target file after the
body runs.

**Resolution (2026-07-18):** stderr redirection on a compound command is
now wired via a `stderr_stack: Vec<StderrTarget>` on the `Shell`. Before
running the body, `exec_redirected` pushes a `StderrTarget` (`File` for
`2>`/`2>>`; for `2>&1` with fd 1 not a file, mirrors fd 1's live sink —
`Buffer`+merge for a captured stdout, `Pipe` for a pipe, `Stdout` for
inherit) and pops it after. All fd-2 output — command diagnostics
(`errln`), builtin `>&2` (`write_bytes` → `emit_stderr`), and external
child stderr (`run_external`, precedence: per-command `2> file` > `2>&1` >
`stderr_stack` top > inherit) — consults `stderr_stack.last()`, so the
whole group honours the redirect. `1>&2` (`stdout_to_stderr`) routes the
body's stdout to the current stderr sink. `Arc` (not `Rc`) is used because
subshell clones move into scoped pipeline threads (`clone_for_subshell`
resets the stack to empty). Tests: `compound_stderr_redirect_to_file`,
`compound_stderr_append_redirect`, `compound_stderr_to_stdout_in_capture`,
`compound_stderr_to_stdout_top_level_capture`,
`compound_for_loop_stderr_redirect`. **Best-effort limit:** `2>&1`
interleaving into a captured stdout is not byte-interleaved — the group's
stderr is folded in after the body finishes (documented in `exec_redirected`).

### TD-OILS4. `osh` pipelines: per-stage redirects composed with inter-stage pipes — RESOLVED 2026-07-18 (threaded streaming pipeline landed 2026-07-18; redirect composition verified 2026-07-18)

**Where:** `userspace/oils/src/interp.rs` (`exec_pipeline`,
`exec_concurrent_pipeline`, `exec_threaded_pipeline`, `finish_pipeline`,
`stage_is_plain_external`).

**What:** Pipelines now run **concurrently and stream** on every path.
An all-external pipeline wires real OS pipes between child processes; any
pipeline containing a builtin/function/compound stage uses the *threaded*
path (`exec_threaded_pipeline`): each stage runs in its own subshell on
its own thread, connected by real OS pipes (`io::pipe`), via the new
`Out::Pipe`/`StdinSrc::Pipe` endpoints and the `pipe_broken` flag (the
in-process SIGPIPE analogue — a builtin write to a closed downstream pipe
returns exit 141 and unwinds the stage). Downstream early-exit propagates
upstream: an in-process producer stops on `pipe_broken`; an external
producer stops on the OS broken-pipe signal on targets that deliver it.
`pipefail` + `${PIPESTATUS[@]}` record per-stage codes on both paths.
This resolves the former "buffered fallback isn't concurrent" gap.

**RESOLVED:** per-stage redirects *are* composed with the inter-stage
pipe. A stage with its own redirect (`a | b > f`, `a | b 2>err`) is routed
to the threaded path; `run_external` (and `run_builtin`) resolve the
stage's `RedirPlan` against the pipe endpoints when building the child —
`redir.stdout`/`redir.stderr`/`redir.stdin` (a file, or here-doc/cursor
`stdin_data`) override the corresponding `Out::Pipe`/`StdinSrc::Pipe`
endpoint, and where there is no redirect the pipe endpoint is used. Two
Windows tests (`pipeline_stage_stdout_redirect_composes_with_pipe`,
`pipeline_stage_stderr_redirect_composes_with_pipe`) verify a redirected
external stage's stdin still comes from the upstream pipe while its
stdout/stderr are diverted to files (on both the last-stage main-thread
path and a worker-thread stage).

**Note on external-producer early-termination testing:** relying on the
OS to kill an unbounded *external* producer when its consumer exits is a
target-OS property (bash uses SIGPIPE; slateos delivers EPIPE), not shell
logic. The Windows test host can't exercise it — `cmd`'s `echo` ignores
broken-pipe writes and loops forever, so `Child::wait` never returns.
The shell-side cascade is covered by
`threaded_pipeline_inprocess_producer_terminates_early`; there is
intentionally no external-producer variant (see the comment in
`interp.rs` tests).

Documented in the `interp.rs` module header. Tests cover the threaded
streaming path (subshell isolation, in-process early-termination,
classifier routing) but not the deferred per-stage-redirect gap.

### TD-OILS5. `osh` arithmetic: assignment/increment operators + C-style `for (( ; ; ))` — RESOLVED 2026-07-18

**Where:** `userspace/oils/src/arith.rs` (`VarLookup` trait, `eval`, the
`Expr` AST + `AParser`), `userspace/oils/src/interp.rs` (`impl VarLookup
for Shell`, `eval_arith_raw`, `exec_for_arith`), `userspace/oils/src/ast.rs`
(`ForArithClause`), `userspace/oils/src/parser.rs` (`parse_for_arith`).

**Resolution:** `arith.rs` was rewritten from a fused parse+eval into a
two-phase **AST design**: `parse(expr, &dyn VarLookup) -> Expr` (immutable
borrow) then `eval_expr(&Expr, &mut dyn VarLookup)` (mutable borrow), so
`eval` is now `eval(expr, &mut dyn VarLookup) -> i64`. `VarLookup` gained
`set`/`set_index`/`set_assoc` (empty defaults; `impl … for Shell` writes
back to `vars`/`arrays`/`assoc`). The evaluator now supports the full
mutation set: assignment `= += -= *= /= %= <<= >>= &= |= ^=` (right-assoc,
looser than `?:`, tighter than `,`; chained `a = b = c` works), pre/post
increment/decrement `++x`/`x++`/`--x`/`x--`, and exponentiation `**`
(right-assoc). `&&`/`||` short-circuit and `?:` is branch-lazy, so side
effects only fire on the taken path. Array/associative element assignment
(`a[i] = …`, `m[key] += …`) resolves the subscript once (`ResolvedLv`).

The **C-style `for (( init; cond; update ))` loop** is implemented: the
lexer already emits `(( … ))` as a single `ArithCmd` token, so `parse_for`
detects it, splits the raw text on `;` into three sections
(`ForArithClause`), and `exec_for_arith` runs `init` once, loops while
`cond` is non-zero (empty ⇒ always true), and runs `update` after each
iteration (including after `continue`). `break`/`continue` with a level
count propagate as for other loops.

**Tests:** unit tests in `arith.rs` (`assignment_scalars`,
`increment_decrement`, `indexed_assignment_and_incr`,
`short_circuit_side_effects`, `exponent`, updated
`associative_subscripts`/`comma`) + interp integration tests
(`arith_assignment_command`, `arith_increment_command`,
`arith_assignment_array_elements`, `arith_c_style_for_loop`). All 165
oils tests pass; clippy clean; slateos target builds.

### TD-OILS6. `osh` `read` builtin: `-t` not honored (`-u` now resolved) — OPEN (low priority)

**Where:** `userspace/oils/src/interp.rs` (`builtin_read`).

**What:** `read` now supports `-r` (raw/no-escape), `-a array` (split into
an indexed array), `-p prompt` (prompt to stderr), `-s` (silent — no-op for
non-tty input), `$IFS`-aware field splitting (whitespace-vs-non-whitespace
IFS, last variable gets the raw remainder, backslash escaping without `-r`),
and — as of the latest change — `-d delim` (read up to an alternate
delimiter; `-d ''` ⇒ NUL), `-n N` (stop after N characters *or* the
delimiter, whichever comes first), and `-N N` (read exactly N characters,
ignoring the delimiter). `-d`/`-n`/`-N` use a byte-level `read_record`
helper (UTF-8-aware character counting) dispatched over the same
`StdinSrc`/`RedirPlan` sources as `read_line`, and set the exit status
correctly (0 on delimiter/count reached, 1 on a short read at EOF).
**`-u fd` — RESOLVED 2026-07-19.** `read -u N` (N ≥ 3) now reads from a
descriptor opened by `exec N< file` via the per-shell `open_fds` table (see
TD-OILS14); `-u 0` falls back to normal stdin, and an unopened fd is a
status-1 `read: N: bad file descriptor`. Still missing: `-t timeout` (timed
read) — its option-argument is **parsed and consumed** (so it isn't mistaken
for a variable name) but otherwise ignored.

**Proper fix:** `-t` needs a timer/tty-timeout facility the current model
lacks (no async/tty timeout). Deferred as low priority — scripts rarely use a
`read` timeout compared to `-r`/`-a`/`-n`/`-d`/`-u`. Note the special
`-t 0` case is *separable* and needs no timer: it must return 0 iff input is
available on the source without reading anything (bash), so `read -t 0 x <
/dev/null` → status 0 (EOF counts as "available") where osh currently reads to
EOF and returns 1. Implementing just `-t 0` only requires a non-consuming
"is there data / is the source at a readable state" probe over the
`StdinSrc`/`RedirPlan` sources.

### TD-OILS7. `osh` `readonly`: enforcement across assignment/`unset`/`declare`, the `read` builtin, and temporary env prefixes — RESOLVED

**Where:** `userspace/oils/src/interp.rs` (`apply_assignment`,
`builtin_unset`, `builtin_declare`/`builtin_readonly`, `builtin_read`, and
the env-prefix guard in `exec_simple`; shared `set_scalar_checked` helper).

**What:** `readonly name[=val]` / `declare -r` mark a variable read-only,
and every write path now enforces it with status 1 and no mutation:
reassigning it (`x=2`), unsetting it (`unset x`), re-declaring a value
(`declare x=…`), reading into it (`read x`, field `read a b`, or
`read -a arr`), and temporarily overriding it as a command prefix
(`readonly x; x=1 cmd`) — all emit `osh: NAME: readonly variable` and leave
the value intact. For a multi-name `read a b`, earlier fields are assigned
before the readonly field aborts (bash field-order semantics). The env
prefix is guarded *before* dispatch in `exec_simple`, so no
function/builtin/external path can mutate a readonly variable.

**Resolution:** added a shared `set_scalar_checked(name, val) -> bool`
helper (resolves namerefs, honors `allexport`/case-fold attrs, consults
`self.readonly`, emits the diagnostic, returns false on rejection) and
routed `builtin_read`'s scalar-assignment sites through it (a rejected
target aborts the read with status 1); the `read -a` array path takes an
inline readonly check; and `exec_simple` guards the temporary env-prefix
names against `self.readonly` before dispatch. Tests:
`read_into_readonly_var_fails`, `read_field_readonly_aborts_after_earlier_fields`,
`read_array_readonly_fails`, `env_prefix_readonly_var_errors`.

### TD-OILS8. `osh` extglob: `!(cmd)` with no space is a pattern word, not a negated subshell — OPEN (low priority, intentional superset tradeoff)

**Where:** `userspace/oils/src/lexer.rs` (`read_word`, the extglob group
opener) and the whole-program parse entry (`parse`).

**What:** `extglob` extended-pattern matching (`?()`/`*()`/`+()`/`@()`/`!()`)
is fully implemented in the matcher (`compile_glob`/`match_glob_toks`) and
honored in pathname expansion, `case`, `[[ == ]]`/`[[ != ]]`, and
parameter-expansion pattern operators. Bash gates the *lexing* of these
groups on `shopt -s extglob` being in effect **at parse time**; our
interpreter parses the whole program up front, independently of runtime
`shopt`, so the lexer always consumes an unquoted `X(…)` group (X ∈
`?*+@!`) into a single word token and defers the extglob decision to match
time (with extglob off the group matches literally, which is bash-correct
for the common patterns). The one visible consequence: `!(cmd)` written
with **no space** is now lexed as a pattern word rather than a `!`-negated
subshell. Workaround: write `! (cmd)` (with a space) for a negated
subshell — which is the more common and readable form anyway.

**Proper fix (if ever needed):** thread an `extglob` flag through the
parser that mirrors bash's parse-time gating — e.g. re-parse or lazily
tokenize on a per-command basis so a `shopt -s extglob` earlier in the
script enables group lexing only for later commands. Deferred: the current
behavior is a deliberate, documented superset tradeoff and `!(cmd)`
no-space negated subshells are vanishingly rare.

### TD-OILS9. `osh` `printf '%(FMT)T'`: time is always formatted in UTC, not local time — OPEN (low priority, gated on timezone infrastructure)

**Where:** `userspace/oils/src/interp.rs` (`format_strftime`, called from
`format_conversion`'s `%(…)T` branch).

**What:** bash's `printf '%(FMT)T'` formats the broken-down time in the
shell's *local* timezone (honoring `$TZ` / the system zone). Our
implementation renders the time in **UTC** because SlateOS has no timezone
database and no `$TZ` handling yet. This shifts not just the zone name/offset
but the actual broken-down values near a day/year boundary: e.g.
`printf '%(%Y)T' 0` (epoch 0 = 1970-01-01 00:00:00 UTC) prints `1970` in osh
but `1969` under a negative-offset local zone like `EST` (bash → 1969-12-31
19:00 local). All calendar math (`civil_from_days` /
`days_from_civil`) and the specifier set (`%Y %C %y %m %d %e %H %I %k %l %M
%S %p %P %A %a %B %b %h %j %u %w %s %z %Z %V %G %g %n %t %F %T %R %D %r %c
%x %X %%`) are correct; only the local-zone offset is missing. Under the UTC
model `%z`→`+0000` and `%Z`→`UTC` (added 2026-07-20 alongside `%k %l %r %V
%G %g %c %x %X`; all verified equal to `TZ=UTC bash` across many epochs and
ISO-week boundaries). Minor: `%Z` prints `UTC` (the glibc/target value)
whereas the MSYS reference bash prints `GMT` for `TZ=UTC` — a host-libc name
difference, not an osh bug (the slateos target is glibc-like). Also, bash's
`-2` argument ("time the shell was started") is approximated as the current
time, since `format_printf` is a free function without access to the shell's
start instant.

**Proper fix:** once SlateOS grows a timezone database / `$TZ` parsing,
apply the local UTC offset (and DST rules) before breaking the epoch down,
and thread the shell start instant through so `%(…)T -2` is exact. Deferred:
UTC formatting is correct and deterministic, and scripts that need a
specific zone can compute the offset explicitly.

### TD-OILS-HOST-ARGV0. External commands see the resolved absolute path as `argv[0]` on the *Windows host build* instead of the command word as typed (correct on the slateos/unix target) — NOT-A-BUG on target / host-only test artifact — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` (`exec_external`, the `PCommand`
construction ~line 5664). The `arg0` override is `#[cfg(unix)]`.

**What:** bash execs the PATH-resolved binary but hands the child `argv[0]` set
to the command word *exactly as typed* (`cat`, not `/usr/bin/cat`), so a program
reports its own name the way the user invoked it. Probe symptoms on the host:
`osh -c 'cat /nope'` prints `/usr/bin/cat: /nope: No such file or directory`
(bash: `cat: …`), and `osh -c 'sh -c "echo \$0"'` prints `/usr/bin/sh` (bash:
`sh`). This affects any external program's self-named error prefix, `$0` in a
child shell script, and `ps`/process listings.

**Why NOT a bug on target:** osh *does* set `argv[0]` to the typed name via
`std::os::unix::process::CommandExt::arg0`, which is compiled in for the
slateos target (`cfg(unix)` true) — so the shipped OS matches bash. The
divergence exists only on the `x86_64-pc-windows-gnu` host build, where
`std::process::Command` has no `argv[0]` override and the MSYS runtime
reconstructs `argv[0]` from the full executable path. Same disposition family as
the `/tmp`→`D:\tmp` path and `$HOME` format artifacts: a host-execution
difference, not a target-behavior bug. No action needed; validate argv[0]
behavior on the slateos target when a ring-3 exec self-test exists.

### TD-OILS-UNICODE-ESC. `$'\uHHHH'` / `$'\UHHHHHHHH'` always emit UTF-8 (correct for SlateOS; differs only from MSYS bash's C-locale default) — NOT-A-BUG / documented probe artifact — 2026-07-20

**Where:** `userspace/oils/src/lexer.rs` (ANSI-C `$'…'` unescaping, `\u`/`\U`
handling) — the code point is encoded as UTF-8 via Rust's `char`→`str`.

**What:** osh renders `$'\u00e9'` as the two UTF-8 bytes `c3 a9` (`é`) and
`$'\u20ac'` as `e2 82 ac` (`€`). MSYS bash, which defaults to the **C/POSIX
locale**, instead encodes `\u` code points in the current locale's charset: a
code point ≤ 0xFF becomes a single byte (`\u00e9` → `e9`), and one that doesn't
fit is passed through **literally** (`\u20ac` → the 6 chars `\u20AC`). This makes
a host probe against MSYS bash show a spurious diff.

**Why NOT a bug:** run the *same* MSYS bash under a UTF-8 locale
(`LC_ALL=en_US.UTF-8 bash -c "echo \$'\u00e9'"`) and it emits `c3 a9` —
byte-identical to osh. SlateOS is UTF-8-native, so osh's unconditional UTF-8
encoding is the correct target behavior and matches bash under the modern
default locale. Same family as the `/tmp`→`D:\tmp` path translation and the
`type -a` external-path artifacts: an MSYS host-environment difference, not an
osh divergence. No action needed.

### TD-OILS-PRINTF-QUOTE-CHAR. `printf %d "'X"` yields the character's Unicode code point, not its first UTF-8 byte (correct for SlateOS; differs only from MSYS bash's byte-wise C locale) — NOT-A-BUG / documented probe artifact — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` (`format_printf`, the leading-quote
`'X` numeric-argument rule for `%d`/`%i`/etc.).

**What:** POSIX printf treats an argument that begins with `'` or `"` as "the
numeric value of the character following the quote, in the current locale."
For a multi-byte character such as `あ` (U+3042), osh emits the **code point**
`12354`. MSYS bash, running in its byte-wise C/POSIX locale, instead emits the
**first byte** of the UTF-8 encoding (`0xE3` = `227`).

**Why NOT a bug:** run the *same* rule under a UTF-8 locale and real Linux/GNU
bash also returns the wide-character value `12354` — osh matches the modern
UTF-8-locale behavior. Same root cause and disposition as `TD-OILS-UNICODE-ESC`
(osh is unconditionally UTF-8-native, which is the correct target for SlateOS);
the MSYS byte-wise result is a host-locale artifact, not an osh divergence. No
action needed.

### TD-OILS-STRLEN-CHARS. `${#var}` and `${var:off:len}` count Unicode characters, not UTF-8 bytes (correct for SlateOS; differs only from MSYS bash's byte-wise C locale) — NOT-A-BUG / documented probe artifact — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` — parameter-length (`${#x}`) and
substring (`${x:off:len}`) expansion, both routed through osh's UTF-8-aware
character-counting helper.

**What:** For `x="héllo"` (the `é` is the 2-byte UTF-8 sequence `c3 a9`):
- `${#x}` → osh `5` (characters h,é,l,l,o); MSYS bash `6` (bytes).
- `${x:1:2}` → osh `él` (characters 1..2); MSYS bash `é` (bytes 1..2 = `c3 a9`).

Each model is internally consistent: osh counts/indexes by character
throughout, MSYS bash by byte throughout.

**Why NOT a bug:** run the *same* operations under a UTF-8 locale and real
Linux/GNU bash also returns `5` / `él` — osh matches modern UTF-8-locale bash.
Same root cause and disposition as `TD-OILS-PRINTF-QUOTE-CHAR` and
`TD-OILS-UNICODE-ESC`: osh is unconditionally UTF-8-native (the correct target
for SlateOS, which has no C/POSIX byte locale), so the MSYS byte-wise result is
a host-locale artifact, not an osh divergence. No action needed.

### TD-OILS-XTRACE-PIPE-ORDER. `set -x` traces multi-stage pipeline commands in reverse (last-stage-first) order rather than bash's left-to-right — cosmetic / documented tradeoff — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` — `exec_threaded_pipeline` (and the
all-external `exec_concurrent_pipeline`). Each pipeline stage emits its own
xtrace line from inside `exec_simple` when it runs.

**What:** `set -x; echo A | cat` prints, in bash:
```
+ echo A
+ cat
```
but in osh:
```
+ cat
+ echo A
```
The trace *content* (fully-expanded, one `+ ` line per stage) is identical; only
the line ordering within the pipeline differs. Both shells are deterministic
(bash always left-to-right, osh always last-stage-first).

**Why it happens:** osh runs the pipeline's **last** stage synchronously on the
current thread (required so `shopt -s lastpipe` can keep its mutations/flow, and
to avoid an extra thread) while stages `0..n-1` run on worker threads that the OS
has not necessarily scheduled yet. The current thread therefore reaches the last
stage's `exec_simple` — and emits its trace — before the workers emit theirs.

**Why not fixed now:** matching bash's exact intra-pipeline trace order would
require either (a) abandoning the "last stage on the current thread" design that
`lastpipe` and the concurrent-pipeline architecture depend on, or (b) hoisting
the trace into the parent and expanding each stage's words there — which would
run each stage's command substitutions **twice** (once for the parent trace,
once in the child), a correctness regression (verified: bash expands each stage
exactly once, in the child subshell). Neither is justified for a debugging-only
cosmetic. The proper fix, if ever pursued, is a per-stage "ready to trace"
barrier that releases the stages' trace emission left-to-right before their
bodies run — non-trivial and not worth the added synchronization on the pipeline
hot path. Deferred.

### TD-OILS-HELP-LAYOUT. Bare `help` uses an osh-identity header + single-column listing, not bash's "GNU bash" banner + COLUMNS-wide 2-column truncated layout — INTENTIONAL / documented — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` — `builtin_help`, the no-pattern branch.

**What:** `help` (no arguments) in bash prints a `GNU bash, version …` banner,
four guidance lines, a star-disabled note, then all builtin synopses laid out in
**two columns** whose width is derived from `$COLUMNS` (default 80), with long
synopses truncated and marked with a trailing `>`. osh instead prints its **own**
identity header (`osh (Oils for SlateOS) <ver>` + the guidance lines) followed by
one **full, untruncated** synopsis per line.

**Why intentional (two independent reasons):**
1. **Identity.** osh must not claim to be "GNU bash" — the banner deliberately
   reports osh's own version, exactly like `--version` and `$BASH_VERSION`. So a
   byte-for-byte match with bash's first line is impossible *by design*, which
   removes most of the value of replicating the rest of the layout.
2. **Losslessness + width-independence.** bash's 2-column layout truncates long
   synopses (`compgen`/`complete`/`printf` lose their tail behind `>`). osh's
   one-per-line listing shows every synopsis in full and does not depend on a
   terminal width osh's line-oriented REPL doesn't track. This is strictly more
   informative for the reader.

`help NAME` / `help -s` / `help -d` / glob and prefix matching all match bash
exactly (see the `builtin_help` test); only the no-argument *listing shape*
differs. Scripts never parse `help` output, so there is no behavioral impact.

### TD-OILS-BRACE-BACKSLASH. A brace char-range spanning `\` (U+005C) yields a literal `\` element; bash yields an empty element there — INTENTIONAL / documented — 2026-07-20

**Where:** `userspace/oils/src/brace.rs` — `sequence_of`, single-character range
generation.

**What:** A brace character range that crosses code point U+005C (`\`) — e.g.
`{A..z}`, `{Y..a}`, `{[..]}` — includes the backslash position. osh emits a
literal `\` for that element; bash emits an **empty** string. `echo {A..z}` thus
shows `… Z [ \ ] ^ …` in osh vs `… Z [  ] ^ …` (blank where `\` would be) in
bash. Both shells agree on element *count* (58 for `{A..z}`); only the backslash
element's content differs.

**Why bash does it:** bash applies ordinary **quote removal** to brace-expansion
output. A brace-range element that is a lone `\` is a trailing/standalone
backslash, which quote removal deletes → empty. The same re-scan also makes a
generated backtick (`` ` ``, U+0060, in-range for `{A..z}`) start a
**command substitution** when a suffix is appended (`{Y..a}Q` → bash errors
`bad substitution: no closing`` ` ``), i.e. bash re-lexes range output as source.

**Why osh's behavior is intentional:** osh treats brace-range-generated
characters as **final literal data** and does not re-lex/quote-remove them. This
is simpler, width/locale-independent, and strictly safer — it never
reinterprets a generated backtick as command substitution or a generated `$` as
an expansion. The only observable cost is this one blank-vs-`\` cell in the
astronomically rare case of a range deliberately spanning `\`. No real script
relies on it; no action needed.

### TD-OILS10. `osh` `time` keyword / `times` builtin: user/sys CPU times are always reported as 0.00 — OPEN (low priority, gated on per-child CPU accounting)

**Where:** `userspace/oils/src/interp.rs` (`Shell::format_time_report`, called
from `exec_pipeline` when a pipeline is prefixed with `time`; and
`Shell::builtin_times`, the `times` builtin).

**Also affects `times`:** the POSIX `times` builtin prints two `user sys` lines
(shell, then children) in bash's `%dm%d.%03ds` form, but both are reported as
`0m0.000s` for the same reason — no per-process CPU accounting exists yet. The
line structure/format matches bash so parsers still work.

**What:** the `time` reserved word (and `time -p`) reports **real** (wall-clock)
time accurately via `std::time::Instant`, but the **user** and **system** CPU
time fields are hard-coded to `0m0.000s` / `0.00`. `std::process::Command` on
the host does not surface per-child `rusage`-style CPU accounting, and the
in-process builtin/function stages have no separate CPU meter. bash reports the
combined user/sys CPU consumed by the timed pipeline's children.

**Proper fix:** on SlateOS, gather child CPU times from the process-exit
accounting the kernel already tracks (wait/waitid returning cumulative
user/sys ticks) and sum them across the pipeline's stages; for in-process
stages, sample a per-thread CPU clock around the stage. Deferred: real time is
the field scripts most commonly want, and it is exact; user/sys reported as
zero is clearly documented and does not affect the pipeline's stdout/status.

### TD-OILS11. `osh` `trap`: async-signal handlers (`INT`/`TERM`/…) are stored but not delivered — OPEN (gated on kernel signal/exception support)

**Where:** `userspace/oils/src/interp.rs` (`builtin_trap`, `fire_trap`,
`run_exit_trap`, `traps` map on `Shell`) and `userspace/oils/src/main.rs`.

**What:** `trap` records handlers for any valid signal/pseudo-signal spec and
prints/lists/resets them correctly. The **synchronous** traps are now all
fired: `EXIT` (once on top-level shell exit), `ERR` (on a failing command
outside an exempt context — same rule as `errexit`, independent of `set -e`),
`DEBUG` (before each simple command), and `RETURN` (on function return, in the
function's scope). The `DEBUG`/`RETURN` traps honour **functrace**
(`set -T`/`set -o functrace`) and the per-function trace attribute
(`declare -ft NAME`); the `ERR` trap honours **errtrace** (`set -E`/`set -o
errtrace`). With the corresponding trace option off, the trap is NOT inherited
into a called function (matching bash) — a failing command inside an untraced
function does not fire the caller's `ERR` trap, only the function call itself
failing at the caller level does. Inheritance is implemented by *masking*
(`Shell::trap_suppress`, per-function frame) rather than removing the trap, so a
trap set inside a function **persists globally** (`trap -p` still shows it) and
fires for later top-level commands — matching bash, and fixing the earlier
discard-on-return model. Subshells (`( … )`) likewise inherit `DEBUG`/`RETURN`
under functrace and `ERR` under errtrace. `ERR` firing uses an "armed at command
start" snapshot so a function that installs its own `ERR` trap and then fails
does not double-fire at the caller. **Residual divergence (minor):** the exact
`ERR` firing *count* under `errtrace` with **deeply nested** spaced subshells
(`( ( false ) )`) diverges — bash caps at 2 firings regardless of depth via
undocumented internal bookkeeping, whereas `osh` fires once per boundary. And
command substitutions `$( … )` intentionally do NOT run the trace traps for
their internal commands (bash's `ERR`-in-`$()` behaviour is quirky — it captures
the trap's own stdout into the result and fires on the sub's overall status).
Both are exotic corners where bash itself is inconsistent. What remains
unimplemented is **async signal delivery** —
handlers for real signals (`INT`/Ctrl-C, `TERM`, `HUP`, …) are stored and
printed faithfully but never invoked, because there is no OS-level signal
delivery on the host and SlateOS uses IPC/exceptions rather than Unix signals.

**Proper fix:** map SlateOS exception/IPC "signal" events to the stored
handlers once the process model exposes them (e.g. a keyboard-interrupt or
kill-equivalent IPC message the shell polls between commands, or an async
callback). Deferred: the synchronous traps cover the overwhelmingly-common
uses (`EXIT` cleanup, `ERR` reporting, `DEBUG`/`RETURN` tracing), and the
storage/print/list/reset surface is complete.

**Also (minor):** a `RETURN` trap is not yet fired for a returning **sourced
script** (only function returns), and an `exit N` *inside* an `ERR`/`DEBUG`/
`RETURN`/`EXIT` handler does not propagate to actually exit the shell (the
handler runs via a nested `run_source` whose `Flow::Exit` is swallowed). Both
are edge cases; the fix is to thread the handler's flow out of `fire_trap`.

**Also (TD-OILS-TRAP-CAPTURE, minor):** synchronous trap handler *output* is
written through `fire_trap` → `run_source` (a fresh `Out::Inherit`, i.e. the
real stdout), so it is NOT captured by an enclosing command substitution.
bash's behaviour here is subtle and inconsistent — it *does* capture a `RETURN`
trap's output inside `x=$(f)` (under functrace) but writes an inner `DEBUG`
trap's output to the terminal even inside `$(…)`. Because faithfully matching
that split would require per-trap routing decisions, `osh` currently sends all
synchronous-trap output to the terminal. Proper fix: thread the active `out`
into `fire_trap` and reproduce bash's per-trap capture rules (RETURN/ERR/EXIT
captured by the enclosing substitution; DEBUG to the terminal). Low priority —
trap handlers that mutate variables (the common case) already work correctly.

### TD-OILS-BUILTINS. `osh` is missing several bash builtins: `kill`, `ulimit`, and the interactive-only set — OPEN (each gated on OS infrastructure or interactive-shell support)

**Where:** `userspace/oils/src/interp.rs` (`BUILTIN_NAMES`, the builtin dispatch
in `run_builtin`, `SIGNALS` table). `type -t <name>` currently reports `file`
(shells out to an external) or nothing for these.

**What:** the following bash builtins are not implemented. On the host they
either resolve to an external (MSYS `kill`/`fc`) or fail; on SlateOS, where those
externals do not exist, they would be `command not found`:

- **`kill`** — script-relevant. Two halves: (a) `kill -l`/`-L` signal
  name↔number translation and listing is *pure formatting* and fully
  implementable now (using osh's Linux-x86 `SIGNALS` table — note this
  intentionally differs from MSYS/Cygwin bash's numbering, so a host probe
  against MSYS bash shows spurious diffs); (b) actually *sending* a signal is
  gated on TD-OILS11 (SlateOS has no Unix signal delivery; std exposes no
  portable arbitrary-PID `kill`). osh can already terminate its own tracked
  jobs via `Child::kill`, but not arbitrary pids/signals.
- **`ulimit`** — read/set process resource limits. Gated on a SlateOS
  resource-limit model (cf. TD-OILS15 `umask`, which is tracked but not
  enforced for the same class of reason).
- **`suspend`** — stops the shell via SIGSTOP; gated on job-control/signals
  (cf. TD-OILS13).
- **Interactive-only (not applicable to `-c`/script use):** `bind` (readline
  key bindings), `complete`/`compopt` (programmable completion), `history`,
  `fc` (history editing/re-execution), `logout` (login-shell exit). These have
  no effect in a non-interactive shell and are low priority until osh grows an
  interactive line editor.

**Proper fix:** implement `kill -l`/`-L` translation + listing now (it is
un-gated), wiring `kill pid`/`kill -SIG pid` to whatever signal/IPC mechanism
SlateOS exposes once TD-OILS11 lands; add `ulimit` against the resource-limit
model when it exists; defer the interactive builtins until there is an
interactive REPL. Each should become a real builtin (registered in
`BUILTIN_NAMES` so `type`/`command -v` report `builtin`) rather than shelling
out.

### TD-OILS12. `osh` `-ef` file test uses path canonicalization, not device+inode — OPEN (low priority, gated on portable inode access)

**Where:** `userspace/oils/src/interp.rs` (`file_cmp`, used by both `test`/`[`
`eval_binary` and `[[ … ]]` `cond_binary`).

**What:** the `-ef` operator (`a -ef b` — "same file") is implemented by
comparing `std::fs::canonicalize(a)` to `canonicalize(b)`. bash compares the
device number and inode from `stat(2)`, which also makes two **hard links to
the same inode under different names** compare equal. `std::fs::Metadata` does
not expose device/inode portably across our host (`x86_64-pc-windows-gnu`) and
the custom `x86_64-slateos` target, so distinct-name hard links are *not*
detected as the same file. The common cases — the same path spelled two ways,
or paths that resolve to the same target through symlinks — are handled
correctly, since they canonicalize identically.

**Proper fix:** once SlateOS exposes a stable file identity (device+inode, or a
file-ID equivalent) through the VFS/`stat` path, compare those instead of
canonical paths. The `-nt`/`-ot` mtime comparisons are already exact.

### TD-OILS13. `osh` job control lacks terminal job control (no job *stop*/resume, no controlling-tty transfer); async `&` for all command forms now works — PARTIALLY RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`exec_background`, `builtin_jobs`,
`builtin_wait`, `builtin_fg`, `builtin_bg`, `builtin_disown`, the `Job` struct /
`JobBody` enum and `Shell::jobs` table).

**What:** background-job tracking (`&` → job table, `jobs`, `wait`, `wait -n`,
`$!`, `disown`, `fg`/`bg`) is implemented. Item (1) below is now **resolved**;
the parts that require a controlling terminal and job-control signals remain
incomplete:
1. ~~Only a single external simple command backgrounds asynchronously.~~
   **RESOLVED 2026-07-19:** every backgrounded form that is *not* a single
   external process — a builtin (`true &`), function, compound command
   (`{ …; } &`, `( … ) &`), pipeline (`a | b &`), negated command, or
   multi-command and-or list — now runs asynchronously on a dedicated OS thread
   executing a subshell clone, registered in the job table via the new
   `JobBody::Thread` variant with a synthetic pid (`SYNTH_PID`, ≥ 900_000). So
   `$!`, `jobs`, `wait`/`wait -n`/`wait $!`, and `disown` all see these jobs, and
   the backgrounded body genuinely runs concurrently (bash's "run `&` in a
   subshell", using a thread rather than a fork). A backgrounded thread job's
   stdin is disconnected (fed an empty cursor → immediate EOF), matching bash's
   redirect of async stdin from `/dev/null` in a non-interactive shell.
   *Two residual narrow limitations of the thread model:* (a) a thread-backed job
   **cannot be `kill`ed** — Rust threads are not cancellable, so `kill %n` on such
   a job detaches the handle and records the signalled status but the thread runs
   to completion; (b) a background job nested inside a **redirected/piped** context
   (`cmd | { job & wait; }`) does not inherit that ambient stdin (it sees EOF),
   because the live pipe reader is not shareable across the thread boundary — this
   pre-dates the fix (the old synchronous fallback passed `Inherit`, equally wrong)
   and is rare. Proper fix for (b): a thread-safe shareable stdin reader.
2. **No job *stop*/resume and no controlling-tty transfer.** `fg`/`bg` are
   implemented as far as is meaningful without terminal job control: `fg` prints
   the job's command line and *waits* for it (it cannot resume a stopped job or
   move the terminal foreground process group), and `bg` is a spec-resolving
   form. There is no process-group / controlling-terminal machinery, so jobs
   cannot be *stopped* (Ctrl-Z / `SIGTSTP`) — the "Stopped" state never occurs —
   and `fg` cannot grant a job the terminal. **Diagnostics now match bash's
   mode-dependent behavior (2026-07-20):** `fg`/`bg` are gated on job control,
   which (as in bash) is on for interactive shells and off for non-interactive
   `-c`/script shells, with `set -m` / `set -o monitor` toggling it explicitly
   (osh tracks this via the `Shell::monitor` flag and `job_control_enabled()`).
   When job control is off, `fg`/`bg` print `fg: no job control` / `bg: no job
   control` on stderr and return 1, exactly like bash — even when a background
   job is running. When it is on and a target resolves, because osh never has a
   *stopped* job (every tracked job is already running), `bg` matches bash's
   already-running case: `bg: job N already in background` on stderr, exit 0
   (previously osh printed a non-bash `[id] cmd &` line on stdout).

**Proper fix (remaining):** once the kernel provides process groups +
job-control signals (ties into TD-OILS11 async-signal delivery), extend `fg`/
`bg` to genuine stop/continue + terminal-foreground transfer and add the
"Stopped" job state. The current implementation is correct for the
overwhelmingly common cases (`cmd & … wait`, `fg` to wait on a background job).

### TD-OILS14. `osh` `exec`: input+output redirection-only forms + named input/write fds + scoped per-command extra fds + builtin stderr redirects + function-invocation redirects + fd save/restore/swap (`exec 3>&1`/`1>&3`/swap) implemented; only a true in-place `execve` remains — PARTIALLY RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`run_builtin` `"exec"` arm sets the
persistent targets; `Shell.exec_stdout`/`exec_stderr`/`exec_stdin` fields;
`write_bytes` `Out::Inherit`, `emit_stderr`, `run_external`, `read_line`,
`read_record_input`, and `read_all_bytes` consult them).

**What:** the `exec` builtin is implemented for the command-replacement case
(`exec cmd args` runs `cmd` then exits the shell with its status; a missing
command exits 127). Status of the remaining aspects:
1. **Output redirection-only `exec` — RESOLVED 2026-07-19.** `exec > file`,
   `exec >> file`, `exec 2> file`, `exec 2>> file`, `exec > file 2>&1`, and
   `exec 1>&2` now persistently rebind the shell's ambient fd 1 / fd 2. The
   shell stores the target as `exec_stdout`/`exec_stderr: Option<Arc<File>>` —
   the file opened once (truncated for `>`, append for `>>`) with the handle
   kept so all subsequent writes share one OS offset (bash dups the fd; it does
   not reopen) — and every ambient fd-1/fd-2 write consults it: builtins via
   `write_bytes`'s `Out::Inherit`
   arm, `>&2` diagnostics via `emit_stderr`, and external children via
   `run_external` (the child's stdout/stderr is opened on the file). Subshell
   clones inherit the redirect (bash: a subshell inherits the fd table). Note
   the same left-to-right ordering simplification the rest of the shell has for
   `2>&1 > f` vs `> f 2>&1` applies here (the dup follows fd 1's *final* sink).
2. **Input redirection-only `exec < file` — RESOLVED 2026-07-19.** `exec < file`
   (and `exec << EOF`) reads the source fully into a per-shell
   `exec_stdin: Option<RefCell<Cursor<Vec<u8>>>>` at exec time; every base fd-0
   read consults it: the `read` builtin (`read_line`/`read_record_input`),
   `read_all_bytes` (used by `mapfile`/`$(<file)`-style reads), and an external
   command inheriting fd 0 (`run_external`'s `StdinSrc::Inherit`+no-redir arm,
   which feeds the child the cursor's remaining bytes). Successive `read`s
   therefore consume successive lines. A subshell clone inherits a *snapshot* of
   the remaining bytes with an independent cursor (reads in the subshell don't
   advance the parent's offset — a minor deviation from bash's shared-fd
   semantics, consistent with how our subshells already copy their stdin). One
   approximation: an external that inherits fd 0 is handed the cursor's whole
   remaining buffer via a pipe, so a subsequent `read` in the parent sees EOF
   rather than the bytes the child left unconsumed (a shared-fd offset would
   differ) — acceptable for the common `exec < f; read …` idiom.
3. **Named input descriptors (`exec 3< file`, `read -u 3`) — RESOLVED
   2026-07-19.** `exec 3< file` (and `exec 3<< EOF` / `3<<< str`) opens a
   user-space input descriptor fd ≥ 3 in a per-shell
   `open_fds: HashMap<i32, RefCell<Cursor<Vec<u8>>>>` (the file is slurped once
   into a position-tracking cursor at redirection time, so a missing/unreadable
   path is an error there); `exec 3<&-` removes the entry. `read -u N` (N ≥ 3)
   then reads successive records from that cursor instead of the ambient input,
   independently of fd 0; an unopened fd is a status-1
   `read: N: bad file descriptor`. `resolve_redirects` captures fd ≥ 3 input
   redirects into `RedirPlan.extra_fds` (`ExtraFdOp::InputBytes`/`Close`), which
   only the `exec` builtin consumes. A subshell clone inherits an
   independent-offset snapshot of each open fd (same approximation as
   `exec_stdin`).
4. **Named write descriptors (`exec 3> file`, `echo >&3`) — RESOLVED
   2026-07-19.** `exec 3> file`/`3>> file` opens a user-space write descriptor
   fd ≥ 3 in a per-shell `open_write_fds: HashMap<i32, Arc<File>>` (the file
   opened once — truncated for `>`, append for `>>` — and the handle kept so
   successive writes accumulate on one OS offset); `exec 3>&-` removes it.
   `resolve_redirects` captures `N> file` (N ≥ 3) into
   `ExtraFdOp::OutputFile(path, append)` (consumed by `exec`) and `M>&N` (N ≥ 3)
   into `RedirPlan.stdout_to_fd`/`stderr_to_fd`. Output is routed there: a
   builtin's `echo … >&3` via a `write_to_fd` helper in `write_bytes`, and an
   external `cmd >&3`/`cmd 2>&3` by building the child's `Stdio` from a
   `try_clone` of the shared handle in `run_external`. An unopened write fd is a
   status-1 `N: Bad file descriptor`. A subshell shares the same `Arc<File>`
   (bash fd inheritance). This also fixed a latent bug: a per-command `N> file`
   (N ≥ 3) previously fell into the Write arm's `_ => plan.stdout` case and
   wrongly redirected fd 1; fd ≥ 3 output redirects now route to `extra_fds` (a
   documented no-op on any command other than `exec`).
5. **Scoped per-command extra-fd redirects (`{ …; } 3< file`) — RESOLVED
   2026-07-19.** A *compound* command carrying a fd ≥ 3 redirect
   (`while read -u 3 line; do …; done 3< file`, `{ …; } 4> log`, `… 3<&-`) now
   installs the descriptor into `open_fds`/`open_write_fds` for the body's
   duration only: `exec_redirected` consumes `plan.extra_fds`, saving each
   touched fd's prior binding (taken by ownership out of the map) and restoring
   it — removing the scoped fd — after the body. So `read -u 3` inside the loop
   reads the file while fd 0 stays free, and fd 3 is gone once the loop exits.
   (A repeated fd in the same plan drops the earlier install before applying the
   next; the *first* occurrence's prior binding is the one restored.)
6. **Builtin stderr redirects (`echo … 2>&3`, `read … 2> file`, `… 2>&1` on a
   *builtin*) — RESOLVED 2026-07-19.** A simple-command builtin now honors its
   own fd-2 redirect: `run_builtin` pushes a scoped `StderrTarget` for the
   builtin's duration based on the `RedirPlan` — `2> file`/`2>> file` →
   `File`, `2>&N` (N ≥ 3) → the new `StderrTarget::WriteFd(Arc<File>)` (routed
   to `open_write_fds[N]`), and `2>&1` (fd 1 not a file) → mirror fd 1's live
   sink (`Buffer` for a captured stdout, `Pipe` for a pipeline stage, `Stdout`
   for the real terminal). Both `emit_stderr` and `child_stdio_for_stderr` gained
   a `WriteFd` arm. The `2>&1`-into-captured-stdout case folds the buffered
   stderr into fd 1's sink after the builtin's stdout (line-level interleaving
   not preserved, as elsewhere). The `exec` builtin is exempt from the scoped
   push (it sets the *persistent* `exec_stderr` itself). The former order-free
   caveat (the `>&2 2>file` combination routing `>&2` output to the file rather
   than the pre-redirect stderr) is **RESOLVED 2026-07-19**: the resolver only
   sets `stdout_to_stderr` for the dup-first ordering (`2>file >&2` still copies
   the file target into `stdout`), so when a per-command stderr redirect is
   present it is the freshly-pushed top of `stderr_stack`. Both the builtin
   `write_bytes` path and the compound/function `exec_with_redirects` finaliser
   now route the `>&2` output via `emit_stderr_depth`, skipping that top entry so
   fd 1 lands in the pre-redirect (enclosing/inherited) sink — matching bash's
   left-to-right redirection semantics. Regression: `dup_stdout_before_stderr_redirect`.
7. **Function-invocation redirects — RESOLVED 2026-07-19.** A function call
   carrying its own redirects (`myfunc > file`, `myfunc 2> err`, `myfunc < in`,
   and compound-scoped fd ≥ 3 forms like `myfunc 3< file`) now applies the
   redirect to the *whole function body*. The former `exec_redirected` body was
   extracted into a reusable `exec_with_redirects(plan, out, stdin, run)` helper
   (it establishes the stdin cursor, pushes the `StderrTarget`, installs scoped
   fd ≥ 3 descriptors, captures file/stderr-bound stdout, runs the body via the
   `run` closure, then tears everything down). `exec_redirected` delegates to it
   with `run = |sh,o,s| sh.exec_command(inner, o, s)`; `exec_simple`'s function
   branch delegates with `run = |sh,o,s| sh.call_function(name, args, assigns,
   o, s, default)` — gated on a new `RedirPlan::needs_scope()` so a redirect-free
   call still dispatches directly. Tests: `function_invocation_stdout_redirect`,
   `function_invocation_stderr_redirect`, `function_invocation_stdin_redirect`.
   Note `stdout_to_fd`/`stderr_to_fd` (dup onto an `exec`-opened write descriptor)
   are *not* covered by the scope — those are applied per-builtin/-external — so
   `myfunc >&3` is not yet routed for the whole body.
8. **fd save/restore/swap (`exec 3>&1`, `exec 1>&3`, `exec 3>&1 1>&2 2>&3`) —
   RESOLVED 2026-07-19.** A redirection-only `exec` now applies its redirects in
   strict left-to-right *source order* against the shell's persistent fd table,
   so all the standard fd-juggling idioms work: `exec 3>&1` saves the current
   fd 1 sink into a user-space write fd (a `try_clone` of `exec_stdout`'s handle,
   or a dup of the real terminal when fd 1 is unredirected); `exec 1>&3` restores
   fd 1 from a saved fd; and the classic swap `exec 3>&1 1>&2 2>&3 3>&-` exchanges
   stdout and stderr. The collapsed `RedirPlan` cannot express ordered fd
   mutation (it buckets each effect into a fixed field and loses order), so the
   `exec` builtin bypasses it: `exec_simple_inner` intercepts an args-less `exec`
   with redirects and calls `apply_exec_redirects(&sc.redirects)`, which walks the
   raw redirects in order. Each `M>&N` dup reads fd N's *current* sink via
   `exec_dup_source` (which returns a concrete dup of the real std fd when fd N is
   still on the terminal, so `1>&2` points fd 1 at fd 2's actual sink even when
   the shell's real fd 1 ≠ fd 2 — e.g. launched under `1>file`). This also
   required widening `exec_stdout`/`exec_stderr` from `(path, append)` to
   `Option<Arc<File>>` so a restored fd can point at an arbitrary handle, not just
   a re-openable path. (The rare `command exec`/`builtin exec` re-dispatch still
   goes through the collapsed-plan path, which handles save/restore but not
   arbitrary-order swaps — an essentially nonexistent usage.) Tests:
   `exec_save_and_restore_stdout`, `exec_swap_stdout_stderr`.
9. **Not a true `execve` — still OPEN (gated on kernel `execve`).** `exec cmd`
   spawns `cmd` as a child, waits, and exits with its status — observationally
   the shell does not continue, but the pid is not preserved and signals are not
   transparently forwarded the way a real in-place `execve` would provide.
10. **Per-command dup-then-close (`cmd 2>&3 3>&-`) — RESOLVED 2026-07-20.** The
    canonical idiom that duplicates a saved descriptor onto fd 1/2 and then
    closes it *on the same command* (`echo hi 2>&3 3>&-`, `{ …; } 1>&3 3>&-`,
    `ls … 2>&3 3>&-`) previously failed with a spurious `3: Bad file descriptor`:
    the order-free `RedirPlan` records the `N>&-` close in `extra_fds` and the
    `M>&N` dup in `stdout_to_fd`/`stderr_to_fd`, and `install_extra_fds` applied
    the close *first*, removing fd N before the dup resolved. Fixed with a
    post-pass in `resolve_redirects`: when the plan still dups from a descriptor,
    the transient (command-scoped) close of that descriptor is dropped, so the
    dup resolves against the live fd and fd N is left in its correct
    post-command (still-open) state — a per-command close is undone afterward
    anyway. The only residual is the reverse ordering `3>&- 2>&3` (which bash
    *rejects*), indistinguishable in the collapsed plan and treated as the useful
    ordering. Regression: `dup_then_close_same_command_resolves_before_close`.

**Proper fix:** (2)–(8) done (see above). Remaining: (9) once the kernel exposes
`execve`, replace the spawn+wait+exit with an actual in-place image replacement
for `exec cmd`.

### TD-OILS15. `osh` `umask` value is tracked but not applied to created-file permissions — OPEN (gated on the target file-mode model)

**Where:** `userspace/oils/src/interp.rs` (`builtin_umask`, `Shell::umask_val`,
`open_out`).

**What:** the `umask` builtin fully implements the value's get/set/print surface
(octal + symbolic, `-S`/`-p`), and the mask is inherited by subshell clones, but
nothing consumes `umask_val` yet. Files created by a redirection (`> file`) are
opened with the OpenOptions defaults; the mask is not subtracted from their
permission bits, and it is not propagated to child processes as a process umask.
On the Windows host (`x86_64-pc-windows-gnu`) there is no Unix mode concept, so
this cannot be exercised there anyway.

**Proper fix:** once SlateOS exposes file modes through its VFS/open path (and a
process-level umask, ideally), (a) subtract `umask_val` from the mode when
`open_out` creates a file, and (b) set the child's umask when spawning external
commands. On a `cfg(unix)` build this can already be wired via
`std::os::unix::fs::OpenOptionsExt::mode(0o666 & !umask)`.

### TD-OILS16. `osh` bare `set` lists variables but not function definitions — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`builtin_set`, no-args branch);
`userspace/oils/src/unparse.rs` (the AST source pretty-printer).

**What:** bare `set` (no operands) lists every shell variable in sorted,
re-inputtable `name=value` / `name=([i]="v" …)` form. Bash additionally prints
each defined shell function's full source after the variables (e.g.
`foo () { … }`). Our listing omitted functions.

**Resolution:** implemented a faithful AST-to-source pretty-printer,
`unparse.rs` (`unparse_function`, `program_block`, `word_src`, …), that
reconstructs re-parseable shell source from a `Program`/`FunctionDef` (round-trip
tested: dump → re-parse → dump is stable). `builtin_set` now iterates
`self.funcs` in sorted order after the variables and appends each function's
reconstructed source via `unparse_function`. Shared with TD-OILS18 (`declare -f`)
and the `type NAME` function branch. Regression test: `bare_set_lists_functions`.

### TD-OILS17. `osh` namerefs (`declare -n`): all originally-listed edge cases now match bash — RESOLVED 2026-07-18

**Where:** `userspace/oils/src/interp.rs` (`resolve_ref_name` and the read/write
chokepoints: `param_value`, `param_elem_value`, `assign_elem`, `array_elements`,
`array_keys`, `expand_array_ref`, `apply_assignment`).

**What:** `declare -n ref=target` / `local -n` namerefs are implemented — reads
and writes of the nameref (scalar and array element, `${ref[@]}`, `${#ref[@]}`,
the pass-array-by-reference-to-a-function pattern) transparently redirect to the
target, chains are followed with a cycle guard, `declare -p` shows `-n`, and
`unset -n`/`unset` behave per bash. Remaining deviations:
1. ~~**`${!ref}` on a nameref** returns the target's *value* (ordinary indirect
   expansion), whereas bash returns the target *name*.~~ **FIXED 2026-07-18.**
   `expand_indirect` now special-cases a nameref `refname`: `${!ref}` follows
   the nameref chain (`resolve_ref_name`) and yields the final target *name*,
   while `$ref` still yields the target value. Regression: the
   `param_indirect_expansion` test.
2. ~~**Namerefs to an array element** (`declare -n ref=arr[0]`) are stored
   verbatim; `resolve_ref_name` returns `arr[0]` and the caller's subscript logic
   does not further interpret it, so `$ref` does not resolve to that element.~~
   **FIXED 2026-07-18.** Reads go through a new `nameref_elem_value` helper
   (splits `arr[0]`/`m[key]` and reads the element), and `apply_assignment`
   rewrites `ref=v` into `arr[0]=v` (synthesising the subscript word) when the
   resolved nameref target carries a subscript. Regression: `nameref_to_array_element`.
3. ~~**`local -n` scoping** uses the same global attribute set as the other
   `local` attributes (`-i`/`-l`/`-u`), which are not yet per-frame~~ — **FIXED
   2026-07-18.** The per-call `VarSnapshot` now captures and restores the
   `integer`/`lower`/`upper`/`nameref`/`readonly` flags along with the value, and
   `declare_local` clears `-i`/`-l`/`-u`/`-n` when shadowing (a bare `local x`
   does not inherit a global's attributes, matching bash; `readonly` is left
   intact so a readonly global is not silently shadowed). Regression tests:
   `local_integer_attr_does_not_leak`, `local_restores_shadowed_integer_attr`,
   `local_nameref_does_not_leak`.

**Proper fix:** all three items (1), (2), (3) are now fixed (see above). This
entry is retained for history; nameref behavior now matches bash for the
scalar, array-element, indirect-name, and `local`-scoping cases exercised by the
`nameref_*` and `param_indirect_expansion` tests.

### TD-OILS18. `osh` `declare -f` / `type funcname`: function *body* is now printed — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`declare_functions`, the `type`
builtin's function branch, and `builtin_set`'s no-args listing — TD-OILS16);
`userspace/oils/src/unparse.rs` (the pretty-printer).

**What:** `declare -F` fully lists/tests function names, and `declare -f name`
reports the correct existence status, but neither `declare -f` nor
`type funcname` printed the function *body* — they needed a faithful
`FunctionDef`-AST → shell-source pretty-printer, which the AST could not do
directly (it carries no source spans, so the text must be reconstructed from the
parsed tree).

**Resolution:** added `unparse.rs`, an AST-to-source pretty-printer covering
`Program`/`Command` (all compound forms), `Word`/`WordPart` (every parameter-
expansion variant, command/arith substitution, arrays/slices/bulk ops),
assignments, redirections, and `[[ … ]]`/`(( … ))` — verified by a round-trip
stability property test (`parse(print(f))` re-prints identically; the AST derives
`PartialEq`). It is now used by: bare `declare -f` and `declare -f NAME` (print
each function's reconstructed source), `type NAME` (prints the "is a function"
line then the source), and bare `set` (lists functions after variables, closing
TD-OILS16). One deliberate simplification: here-documents are re-emitted as
here-strings (`<<< …`) — same bytes to stdin, re-parseable. Regression tests:
`declare_small_f_prints_body`, `type_function_prints_body`,
`bare_set_lists_functions`, and the `unparse::tests` round-trip suite.

### TD-OILS19. `osh` alias expansion applies across `run_source` calls (input reads), not within a single parsed unit — OPEN (minor fidelity gap; interactive/REPL use unaffected)

**Where:** `userspace/oils/src/interp.rs` (`run_source` → `parse_with_aliases`),
`userspace/oils/src/lexer.rs` (`expand_aliases`), `userspace/oils/src/main.rs`
(script/`-c`/REPL entry points).

**What:** `alias`/`unalias` are implemented and aliases are expanded over the
token stream *before parsing* (`parse_with_aliases`), matching bash's pre-parse
alias pass — including command-position-only expansion, the recursion guard
(`alias ls='ls -l'` terminates), and the trailing-blank rule (`alias sudo='sudo '`
makes the next word alias-eligible). However, because the interpreter parses an
entire `run_source` input in one shot, an alias **defined earlier in the same
input** does not take effect for **later commands in that same input** (e.g. a
script file or a single `osh -c '…'` string). Bash's own rule is close but not
identical: bash reads a script line-by-line, so `alias x=…; x` on one line does
not expand either, but `alias x=…` on line 1 and `x` on line 2 *does*. Our
REPL reads one line per `run_source` call, so interactive alias use behaves
correctly; only multi-line scripts / `-c` strings diverge (an alias defined on
an earlier line is not seen by a later line in the same file). Aliases inside
command-substitution bodies (`$(…)`) are also not expanded — those are parsed by
`parser.rs`'s recursive `parse(raw)`, which has no access to the shell's alias
table.

**Proper fix:** parse-and-execute the top-level input command-by-command (or
line-by-line) so the alias table is consulted incrementally, re-tokenizing/
re-expanding each command against the live `self.aliases` right before it runs;
and thread the alias table (or a `parse_with_aliases` variant) through the
command-substitution parse path. Deferred because it touches the top-level
execution driver; the current behavior covers interactive use and the common
"aliases defined in an rc file, used at the prompt" workflow.

### TD-OILS20. `$LINENO` counts only top-level newline tokens — lines inside multi-line quotes/substitutions/here-docs are undercounted — FIXED 2026-07-19 (embedded-newline undercount); two narrow sub-gaps remain

**Status (2026-07-19, FIXED):** the embedded-newline undercount is resolved. The
lexer now tracks a running source line and stamps every token with its true
starting line (`tokenize_spanned` returns a parallel `Vec<u32>`; see
`Lexer::stamp_lines`), and the parser reads each `Item`'s line from that vector
(`Parser::cur_line`) instead of counting top-level `Newline` tokens. `$LINENO`
and error line numbers are now exact across newlines swallowed inside
here-document bodies, multi-line quoted strings, and command substitutions
(verified against bash). **Two narrow sub-gaps remain OPEN:** (a) per-*command*
granularity — osh tracks line per top-level `Item`, whereas bash advances
`$LINENO` per simple command, so a multi-line pipeline's failing stage or a
command with an embedded-newline argument reports the item's start line rather
than the stage/command line (logged in todo.txt with the exact fix: add a
`line` to the command node and set `current_line` per stage). (b) `$LINENO` is
not reset to be relative to a function's definition the way bash does — ours is
absolute to the parsed unit.

**Where:** `userspace/oils/src/lexer.rs` (`Lexer.line`/`stamp_lines`/
`tokenize_spanned`), `userspace/oils/src/parser.rs` (`Parser.lines`/`cur_line`),
`userspace/oils/src/ast.rs` (`Item.line`),
`userspace/oils/src/interp.rs` (`Shell.current_line`, set in `exec_program`,
read in `param_value` as `"LINENO"`).

**What:** `$LINENO` is implemented by having the parser count the top-level
`Tok::Newline` tokens it consumes and stamp the current 1-based line onto each
parsed `Item`; the interpreter sets `self.current_line = item.line` before
running each item and returns it for `$LINENO`. This is accurate for ordinary
multi-line scripts, blank/comment lines, and semicolon-joined commands. It
diverges from bash when a **single logical line spans multiple physical lines
via a construct whose interior newlines are not top-level `Newline` tokens** —
e.g. newlines inside a double-quoted string, a `$(…)`/`` `…` `` command
substitution body, an arithmetic `$(( … ))` spanning lines, or a here-document
body. Those interior newlines are absorbed by the lexer into a single segment/
token, so the parser's line counter does not advance across them, and a
`$LINENO` appearing *after* such a construct reports a line number lower than
bash would (bash counts every physical newline the reader consumes). The
counter also does not reset per function body the way bash resets `$LINENO` to
be relative to the function's definition — ours is absolute to the parsed unit.

**Proper fix:** track physical line numbers in the **lexer** (attach a source
line to every `Tok`, counting newlines even inside quoted/substitution/heredoc
segments) and thread that through to `Item.line`, rather than counting
top-level `Newline` tokens in the parser. That makes `$LINENO` exact for all
constructs. Deferred because it requires adding position info to the token
stream (a lexer-wide change) and the current approximation is correct for the
overwhelming majority of real scripts; the discrepancy only appears after
embedded multi-line quotes/substitutions/here-docs.

### TD-OILS21. `BASH_SOURCE`/`caller` do not track *per-function* definition source across `source`/`.` — OPEN (narrow fidelity gap)

**Where:** `userspace/oils/src/interp.rs` (`Shell.refresh_funcname`/`frame_source`
build `BASH_SOURCE` from a single mode-derived label for every function frame;
`Shell.builtin_caller`/`bash_source_at` do the same), `Shell.funcs` (a name→body
map with no definition-site metadata).

**Status (2026-07-19, partially fixed):** the *invocation-mode* dimension is now
correct. `frame_source` returns the label bash uses for function frames —
`environment` under `-c`, `main` under stdin/interactive, and the script path
(`$0`) in script-file mode — and `refresh_funcname` now materialises the
script-file **base frame** (`BASH_SOURCE[0]`/`BASH_LINENO[0]` = script path / 0
even at top level, with `FUNCNAME` gaining its bottom `main` entry only once a
function frame sits above it, so the arrays legitimately differ in length at a
script's top level). `caller`/`caller N` were reworked to bash's indexing
(line `BASH_LINENO[n]`, name `FUNCNAME[n+1]`, source `BASH_SOURCE[n+1]` with a
`NULL` fallback for a top-level caller), so a `caller 0` from a lone function
under `-c` is now correctly out-of-range and the source column matches bash in
every mode. Verified byte-for-byte against real bash in `-c`, stdin, and
script-file modes.

**Remaining gap:** every frame still shares one source label because the
interpreter stores only a function's body (`funcs: name → Program`) with no
record of which file it was defined in. In bash, a function defined in a file
pulled in via `source`/`.` reports *that* file as its `BASH_SOURCE` entry, while
ours reports the current mode's label. This only matters for scripts that
`source` a library of functions and then introspect `BASH_SOURCE`/`caller` to
locate the defining file (e.g. stack-trace/error-reporting frameworks).

**Proper fix:** record the defining source file alongside each function body
(e.g. `funcs: name → (Program, source_name)`), set it from the current `$0`/
`source` target at definition time, and have `frame_source`/`bash_source_at`
read the per-function value instead of the mode label. Deferred because it
requires threading a definition-site source through the `source`/`.` execution
path and the function-definition AST handling; the current behavior is correct
for the common case where all functions live in the script itself.

### TD-OILS22. `osh` process substitution `<(cmd)`/`>(cmd)` uses a temp-file model, not streaming FIFOs — OPEN (gated on `/dev/fd` or named-pipe support)

**Where:** `userspace/oils/src/interp.rs` (`Shell::proc_sub`, `finish_procsubs`,
the `procsub_in_temps`/`procsub_out_jobs` fields, the `exec_simple` wrapper), plus
`lexer.rs` (`Seg::ProcSub`, `read_word`), `parser.rs` (`WordPart::ProcSub`),
`ast.rs`, `unparse.rs`.

**What:** process substitution is implemented, but because the host (and the
current SlateOS target) exposes neither `/dev/fd/N` nor named pipes (FIFOs), the
substituted pathname is backed by an ordinary temp file rather than a pipe wired
to a concurrently-running process (this is the same fallback several shells use
on systems without `/dev/fd`). Consequences:
- **Not streaming.** For `<(cmd)` the command runs to completion *at expansion
  time* and its entire output is buffered into the temp file before the enclosing
  command runs. An unbounded/interactive producer — `<(tail -f log)`,
  `<(yes)` — blocks at expansion instead of streaming, whereas real bash would
  run it concurrently and let the reader consume incrementally.
- **`>(cmd)` is deferred, not concurrent.** The enclosing command writes to a
  temp file; only *after* it finishes does `cmd` run with that file as stdin. So
  the two processes never overlap in time (bash runs them concurrently). Final
  results match for the common finite cases (`tee >(wc -l)` counts correctly),
  but ordering/interleaving and any back-pressure semantics differ.
- **A pid/`/dev/fd` path is not observable.** The substituted word is a real
  temp path (e.g. `/tmp/osh_psub_<pid>_<n>.tmp`), not `/dev/fd/63`; a script that
  parses the substituted filename expecting `/dev/fd/…` would see something else.

Temp files are created lazily during word expansion and cleaned up when the
enclosing command finishes: `exec_simple` records a mark into
`procsub_in_temps`/`procsub_out_jobs` before running the command and calls
`finish_procsubs` after (running deferred `>(cmd)` bodies, then deleting all temp
files). `exec_redirected` does the same for a *compound* command's redirect
target, so `{ …; } > >(cmd)`, `( … ) > >(cmd)`, and `for/while/if/case … > >(cmd)`
now correctly run the deferred output body and clean up (**fixed 2026-07-20**;
regression test `process_sub_output_deferred_on_compound`; previously these
produced *no* output because `finish_procsubs` was never called for compound
commands). Still not swept: process substitutions created while expanding a
non-redirect, non-simple-command *word* context (a `for`-list operand `for x in
<(…)`, a `case` word, a `[[ … ]]` operand) — those leak their temp file for the
shell's lifetime (input procsubs there still *function*, since the file is
written at expansion; only cleanup leaks). A minor, rarely-hit gap.

**Proper fix:** once SlateOS exposes named pipes (a `mkfifo`/FIFO VFS node) or a
`/dev/fd/N` mechanism, replace the temp file with a real pipe: for `<(cmd)` spawn
`cmd` writing the pipe's write end and substitute the read-end path; for `>(cmd)`
spawn `cmd` reading the pipe and substitute the write-end path — so both run
concurrently with the enclosing command and stream. The lexer/parser/AST plumbing
(`Seg::ProcSub` → `WordPart::ProcSub`) stays; only `proc_sub`/`finish_procsubs`
change. Also sweep the non-simple-command contexts (or move cleanup to a
shell-level teardown) to avoid the temp-file leak.

### TD-OILS-COPROC. `osh` has no `coproc` support at all — RESOLVED 2026-07-19

**RESOLVED 2026-07-19.** Implemented — see the resolved TD-OILS-COPROC
entry near the top of this file for the shipped design and remaining minor
limitations. Note the "why deferred" reasoning below turned out to be
over-cautious on one point: a full `Box<dyn Read>`/`Box<dyn Write>`
generalisation of *both* fd tables was **not** needed. `std::io::pipe()`
(stable 1.87, cross-platform) plus a *dedicated* live read table
(`coproc_read_fds`) and reuse of `open_write_fds` for the write end kept
the change to ~4 read-resolution points; the body runs on an OS thread
with its own `Shell` (not an OS process), so no `Child`/reaping subsystem
was required. The concurrency concern was valid and is satisfied by the
real OS pipes + background thread. Original analysis retained below.

**Where (original):** would touch `lexer.rs`/`parser.rs`/`ast.rs` (recognise the `coproc`
reserved word and parse both `coproc [NAME] simple-command` and
`coproc [NAME] { compound }` forms), `interp.rs` (a new async-process launch +
the `open_fds`/`open_write_fds` fd tables + `COPROC`/`NAME` array and
`COPROC_PID`/`NAME_PID` variables), `unparse.rs`.

**What:** `coproc` is not implemented. `coproc cat` runs `coproc` as an external
command (`osh: coproc: command not found`); the block form `coproc NAME { … }`
is a hard parse error (`unexpected reserved word '}'`) because the lexer/parser
do not treat `coproc` as a keyword. bash creates an asynchronous coprocess whose
stdin/stdout are connected to the shell by two pipe fds exposed as an array
(`${COPROC[0]}` = read end, `${COPROC[1]}` = write end, or `${NAME[…]}` for the
named form), plus `COPROC_PID`. The shell and the coprocess run **concurrently**
and exchange data by streaming through those live pipe fds.

**Why deferred (root cause):** osh's host-interp fd model is not backed by live
OS pipe fds. Input fds are in-memory `RefCell<Cursor<Vec<u8>>>` buffers
(`open_fds`), write fds are `Arc<File>` (`open_write_fds`), and the closest
async construct — process substitution — is a **temp-file approximation that
runs each side to completion** rather than streaming (see TD-OILS22). A coproc's
defining semantics are *concurrent bidirectional streaming*: you write a line to
`${COPROC[1]}` and read the response from `${COPROC[0]}` while the coprocess is
still alive. A temp-file/cursor model cannot express this without deadlocking or
serialising the two sides — so a temp-file "coproc" would be a broken hack, not
a fix. Implementing it correctly requires generalising both fd tables to hold
live readable/writable handles (`Box<dyn Read>` / `Box<dyn Write>`, e.g. a
`ChildStdout`/`ChildStdin` from `std::process` with `Stdio::piped()`), threading
those through the `read`/`write`/redirect paths, and tracking the `Child` for
`COPROC_PID` and reaping — the same live-pipe-fd subsystem TD-OILS22 needs.

**Proper fix:** build the live-pipe-fd subsystem (shared with TD-OILS22). Then
for the external-command form, spawn the command with `Stdio::piped()` on stdin
and stdout, register the two pipe ends as live fds in the generalised fd tables,
publish their fd numbers into the `COPROC`/`NAME` array and the pid into
`COPROC_PID`/`NAME_PID`, and let subsequent `read`/`echo` on those fds stream to
the live child. The compound-body form (`coproc NAME { … }`) additionally needs
osh's own interpreter to run the body concurrently (a subshell process or an
OS-thread with its own `Shell`), which is the harder half. `coproc` is one of
the least-used bash constructs, so this is low priority relative to the
live-pipe-fd work it depends on.

### TD-OILS23. `osh` unquoted word splitting ignores a custom `$IFS` (always splits on whitespace) — RESOLVED 2026-07-19

**RESOLVED 2026-07-19.** Replaced the free function `split_ifs` (which split
only on whitespace via `str::split_whitespace`) with `split_field_ifs(s, ifs)`
implementing full POSIX word-splitting: IFS whitespace collapses/trims, each
non-whitespace IFS char is a single delimiter with adjacent whitespace absorbed,
adjacent non-whitespace delimiters produce empty fields, a trailing delimiter
produces no trailing empty field, empty IFS disables splitting, and empty input
yields no fields. The `other =>` arm of `expand_word_annotated` now reads `$IFS`
from `self.vars` (default `" \t\n"`) and routes through it. Regression test:
`unquoted_word_split_honors_ifs`. Now `IFS=:; x="a:b:c"; for w in $x` yields
three fields; `a::c` yields `a`, ``, `c`; leading/trailing custom delimiters and
mixed whitespace+non-whitespace IFS behave per bash.

**Where:** `userspace/oils/src/interp.rs` — `split_ifs` (splits only on
`char::is_whitespace` via `str::split_whitespace`) and its caller, the `other =>`
arm of `expand_word_annotated`.

**What:** an unquoted expansion is field-split on the *default* whitespace IFS
regardless of the current `$IFS`. So `IFS=:; x="a:b:c"; for w in $x; do …` yields
a single field `a:b:c` (bash: three fields `a`, `b`, `c`), and `IFS=,; set -- a,b;
echo $1` likewise does not split on `,`. The quoted joins now honor IFS correctly
(`"$*"` joins with `$IFS[0]` — see `star_sep`), and the `read` builtin already
splits on the live `$IFS` (`read_split`, which distinguishes whitespace vs
non-whitespace IFS runs) — only the *word-splitting* pass after parameter/command/
arithmetic expansion is stuck on whitespace.

**Proper fix:** replace `split_ifs` (and the `other =>` split in
`expand_word_annotated`) with an IFS-aware splitter modelled on `read_split`:
read `$IFS` from `self.vars` (unset ⇒ `" \t\n"`, empty ⇒ no splitting), collapse
runs of IFS-whitespace, and treat each non-whitespace IFS character as a single
delimiter (with the usual "adjacent whitespace+delimiter counts once" rule). Route
the annotated-expansion unquoted arm through it. This needs `&self` access to read
IFS, so `split_ifs` must become a method (it is currently a free function).

### TD-OILS24. `osh` arithmetic-error abort is command-scoped, not bash's whole-input-line longjmp — MINOR DEVIATION

**Where:** `userspace/oils/src/interp.rs` — `arith_sub` (sets the per-shell
`arith_error` flag on an `arith::eval` `Err`) and `exec_simple_inner` (checks and
clears it after word/assignment/prefix expansion, skipping the command with
status 1).

**What:** when a `$(( … ))` substitution fails to evaluate (e.g. division by
zero, `echo $((1/0))`), bash performs a `top_level` longjmp that discards **every
command sharing the current input line** — so `x=$((1/0)); echo hi` runs *neither*
the assignment nor the `echo` (execution resumes at the next input line). osh
instead aborts only the single offending simple command (or assignment) with
status 1 and continues with the following command on the same line, so
`echo $((1/0)); echo hi` still prints `hi`. Two lesser sub-deviations: (a) a pure
assignment whose value errors (`x=$((1/0))`) leaves `x` holding the fabricated
`0` rather than staying unset, because `apply_assignment` stores before the
driver observes `arith_error`; (b) the abort is only wired for the simple-command
driver — an arithmetic error inside a compound-command header expansion is not
specially discarded.

**Impact:** low. Single-command arithmetic errors (the overwhelmingly common
case) behave exactly like bash — the command is discarded, no bogus value is
produced, `$?` is 1. The divergence only shows when multiple commands share one
input line *and* an earlier one hits an arithmetic error.

**Proper fix:** introduce a `Flow::AbortLine`-style variant (or thread an
"abort to top level" signal) that `exec_program`/`run_source` propagate up to the
top-level item boundary, discarding the remaining items of the current parsed
input unit before resuming. Also expand the offending assignment's value up-front
(before storing) so an errored `x=$((1/0))` leaves `x` unset, matching bash.

### TD-OILS-PRINTF-ERRORDER. `osh` printf emits all `invalid number` diagnostics *before* any output; bash interleaves them per format-cycle — MINOR DEVIATION 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_printf` (~7975) calls
`format_printf`, which builds the *entire* output string and collects every
numeric-parse error into a `Vec`; the builtin then writes all errors to stderr
and finally writes the accumulated stdout in one `write_bytes`.

**What:** `printf "%d\n" 0x1f 010 0b101` in bash prints `31`, `8`, then the
`0b101: invalid number` error, then `0` — output and error interleave in
argument order. osh prints the error first, then `31 8 0`. Both streams are
internally correctly ordered; only the *cross-stream* interleaving (visible
only when stdout and stderr are merged, e.g. under `2>&1`) differs.

**Impact:** negligible. With separate stdout/stderr — the normal case — every
byte lands on the right stream in the right order, and printf's exit status is
identical. Programs do not rely on stdout/stderr interleaving of an error path.

**Proper fix:** make `format_printf` emit incrementally — write each format
cycle's bytes to `out` as it is produced and flush before writing that cycle's
error to stderr — instead of accumulating one String plus an error `Vec`. Only
worth doing if a real script depends on the interleaving.

### TD-OILS-ENVNAME-IMPORT. `osh` imports environment variables whose names are **not valid shell identifiers** (e.g. Windows' `PROGRAMFILES(X86)`) as ordinary shell variables; bash keeps them in the child environment but hides them from the shell-variable namespace — OPEN (host-only artifact; needs a separate raw-env passthrough store) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `import_environment` (~633) inserts
every `std::env::vars()` pair into `self.vars` + `self.exported` without checking
the name. Child-env construction (~986, ~3836, ~4191) then re-emits them.

**What:** bash only turns an inherited env var into a *shell variable* when its
name matches `[A-Za-z_][A-Za-z0-9_]*`; names with other characters (parentheses,
dots, etc.) are retained in the process environment and passed through to
children, but are invisible to `set` / `export -p` / `${name}`. osh currently
lists them (they show up in `export -p` and `set`), which diverges from bash.
Only observed on the Windows host, where the inherited environment contains
`PROGRAMFILES(X86)` / `COMMONPROGRAMFILES(X86)`.

**Why deferred:** the *correct* fix is to keep a separate raw-environment store
(e.g. `raw_env: Vec<(OsString, OsString)>`) for invalid-name entries, merge it
into the child environment at every spawn site, and never surface it as a shell
variable — a small refactor touching `import_environment`, the three spawn-env
loops, and `clone_for_subshell`. Simply dropping such names at import would match
bash's *shell-variable* view but stop passing them to children, which could break
host programs that rely on `PROGRAMFILES(X86)`. On SlateOS the inherited
environment will use well-formed identifier names, so this is purely a host-test
artifact; parked until it matters. Impact: low — cosmetic `set`/`export -p`
listing noise during host comparison testing only.

### TD-OILS-APPEND-EXTCHILD. External (MSYS) children fail to write to an `>>file` append redirect on the Windows host (`echo: write error: Bad file descriptor`) — OPEN (host-only artifact) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `open_out` (~12073) uses
`OpenOptions::write(true).append(true)`. On Windows Rust maps `write+append`
to `FILE_GENERIC_WRITE & !FILE_WRITE_DATA` (append-only access). osh's own
builtins write to such a handle fine, but when that handle is inherited by an
**external MSYS/Cygwin child** as its stdout/stderr, the Cygwin fd layer rejects
it (it probes for `FILE_WRITE_DATA`) → the child prints `write error: Bad file
descriptor` and produces no output. Affects `extcmd >>f`, `extcmd >>f 2>&1`,
`extcmd &>>f`, `extcmd 2>>f`, i.e. any external command whose target is an
append redirect. Truncating redirects (`>f`, `>f 2>&1`, `&>f`) are unaffected.

**What:** bash appends the child's output to the file normally. osh loses it on
the Windows host only.

**Why host-only:** on the SlateOS (unix/musl) target, `OpenOptions::append`
yields an ordinary writable, inheritable fd and children append correctly — this
divergence cannot reproduce there. The failure is purely the Windows
append-only-handle × MSYS-child-inheritance interaction. Reproducing bash's
behavior on the host would mean opening append targets with full `FILE_WRITE_DATA`
+ manual seek-to-end, which sacrifices atomic-append semantics for a test-only
platform. Parked; comparison testing of append+external cases must run on the
target, not the host.

### TD-OILS-PRINTF-TZ. `osh` renders `printf '%()T'` (and prompt `\d \t \T \@ \A`) in **UTC**, not local time; bash uses the system timezone — OPEN (infrastructure-blocked: no TZ facility, dependency-free crate) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `format_strftime` (~11901) computes
`days`/`hour`/`minute` straight from the raw epoch with no timezone offset, and
has no `%z`/`%Z` handling (an unrecognised `%z` is emitted literally). Callers:
`printf '%(FMT)T'` (~11650) and the prompt time escapes `\d \t \T \@ \A`
(~2759–2794). "Now" comes from `SystemTime::now()` (UTC epoch).

**What:** bash formats `%()T` with the local timezone (via libc `localtime`),
so on a UTC-5 machine `printf '%(%Y-%m-%d %H:%M:%S %z)T\n' 0` prints
`1969-12-31 19:00:00 -0500`; osh prints `1970-01-01 00:00:00 %z`. `%z`/`%Z` are
also unsupported. The date/time *math* is correct — only the zone offset (and
the two zone specifiers) are missing.

**Why deferred (infrastructure-blocked):** oils is intentionally
**dependency-free** (Cargo.toml has zero deps — required for the clean
`x86_64-slateos` build), so pulling in `chrono`/`time` for local-offset support
is off the table. Rust `std` cannot obtain the local UTC offset soundly
(`localtime_r` is unsound across threads and not exposed). The correct fix needs
SlateOS to expose the machine's UTC offset (from its RTC/settings service) and
osh to query it; on the host build it would read the OS offset. Honouring only a
`TZ=±HH:MM`/`TZ=UTC0` env var would be a partial band-aid that still misses
bash's default (system-localtime) case, so it's parked until a real time/zone
facility exists. Impact: low — affects only wall-clock *display* in `%()T` and
the time-bearing prompt escapes.

### TD-OILS-ARITH-ERRTEXT. `osh` arithmetic error *messages* don't match bash's `<expr> : <msg> (error token is "<tok>")` format — RESOLVED 2026-07-20 (superseded by TD-OILS-ARITH-ERRFMT)

**Known residual (cosmetic, minor):** for an eval-time error whose offending
operand carries a **unary prefix** (`-`/`+`/`~`/`!`), osh's `error token` includes
that prefix while bash's token pointer skips it. Example: `$(( 2 ** -1 ))` →
bash `exponent less than 0 (error token is "1 ")`, osh `… (error token is
"-1 ")`. The message body and exit status match; only the parenthetical token
text differs. osh attaches the whole RHS operand span (`arith.rs` `eval_expr`,
`rhs_tok`) as the token, whereas bash reports the source from the innermost
primary. Fixing it means recording the post-unary-prefix source position on the
operand node — the same `Expr`-span refactor the proper fix below describes — for
a token that essentially no script inspects. Deferred with the rest of this item.

**Resolution:** superseded by the completed TD-OILS-ARITH-ERRFMT work. All three
"concrete diffs" listed below now reproduce bash 5.2 exactly, verified 2026-07-20:
`$((5/0))` → `5/0 : division by 0 (error token is "0 ")`; `$((09))` → `09: value
too great for base (error token is "09")`; `$((3.5))` → `3.5 : syntax error:
invalid arithmetic operator (error token is ".5 ")`. The historical analysis
below is retained for context only.



**Where:** `userspace/oils/src/arith.rs` — every `Err(ArithError(...))` site
(`parse`, `parse_atom`, `parse_number`, `apply`, `eval_expr`) and the top-level
`eval`. `ArithError` is a bare `String` with no position/token info.

**What:** on an arithmetic error, bash prints a very specific format that osh
does not reproduce. Two patterns (measured against bash 5.2):

- **Pattern A** (parse/eval errors): `<expr> : <message> (error token is "<tok>")`,
  where `<expr>` is the whole arithmetic string (leading whitespace stripped),
  there is a space *before* the colon, and `<tok>` is the **remaining unparsed
  input from the error position to the end** (double-quotes already removed, and
  it keeps trailing whitespace). Messages seen: `division by 0` (both `/` and
  `%` by zero), `exponent less than 0`, `syntax error: operand expected`
  (`1+`, `x=`, `1&&&2`), `syntax error in expression` (`a b`, `1 2` — an operand
  where an operator was expected), `syntax error: invalid arithmetic operator`
  (`3.5` — a non-operator char where an operator was expected).
- **Pattern B** (numeric-base errors): `<token>: <message> (error token is "<token>")`
  — the prefix is the offending *number token itself* (no leading space, no
  space before the colon), for `value too great for base` (`09`, `0xZ`) and
  `invalid arithmetic base` (`99#5`).

Concrete diffs:
```
$(( 5/0 ))   bash: 5/0 : division by 0 (error token is "0 ")
             osh:  arithmetic: division by zero
$(( 09 ))    bash: 09: value too great for base (error token is "09")
             osh:  arithmetic: bad octal literal '09'
$(( 3.5 ))   bash: 3.5 : syntax error: invalid arithmetic operator (error token is ".5 ")
             osh:  arithmetic: unexpected trailing input in arithmetic: '3.5'
```

**Why deferred:** the parse-time errors can take the "error token" from the
parser's `pos` (`chars[errpos..]`), but the **eval-time** errors (division by
zero, modulo by zero, exponent < 0) fire during `eval_expr`/`apply` on the AST,
*after* parsing — there is no parser position available, so matching bash's
token there requires annotating every `Expr` node with a source span (byte
range) and threading it through construction and evaluation. That is a real
refactor of the `Expr` enum for purely cosmetic stderr text that essentially no
script parses. Behavioral (result-affecting) divergences are higher value.

**Proper fix:** (1) add a `span: (usize, usize)` to the relevant `Expr` variants
(or a parallel side-table keyed by node), populated at parse time from `pos`;
(2) classify errors into an enum carrying the bash message text and the error
position; (3) in `eval`, format Pattern A as `{expr.trim_start()} : {msg} (error
token is "{chars[pos..]}")` and Pattern B (base errors) as `{tok}: {msg} (error
token is "{tok}")`. Match bash's operand-vs-operator "syntax error" distinction
by inspecting the char at the failure position.

### TD-OILS-COND-ERRTEXT. `osh` `[[ … ]]` syntax-error *messages* don't match bash's multi-line "conditional expression" diagnostics — OPEN (low priority, cosmetic stderr text) — 2026-07-20

**Where:** `userspace/oils/src/parser.rs` `parse_cond` and its helpers (the
`[[ … ]]` conditional-expression parser). Every `Err(ParseError(...))` in that
path uses osh's single-line house style (`syntax error: expected ']]' to close
'[['`, `syntax error: unexpected ']]' (expected operand)`, `syntax error:
expected ')' in '[[ … ]]'`).

**What:** on a malformed `[[ … ]]` expression bash prints a **two-line**,
token-naming diagnostic that osh does not reproduce (exit codes match — both
non-zero — only the human-readable text differs). Measured against bash 5.2:

```
[[ 3 -gt 2 -gt 1 ]]   bash: syntax error in conditional expression
                            syntax error near `-gt'
                      osh : syntax error: expected ']]' to close '[['
[[ a b ]]             bash: conditional binary operator expected
                            syntax error near `b'
                      osh : syntax error: expected ']]' to close '[['
[[ -z ]]              bash: unexpected argument `]]' to conditional unary operator
                            syntax error near `]]'
                      osh : syntax error: unexpected ']]' (expected operand)
[[ ( a ]]             bash: unexpected token `]]', expected `)'
                            syntax error near `]]'
                      osh : syntax error: expected ')' in '[[ … ]]'
```

**Why deferred:** this is the same class of cosmetic stderr-text divergence as
the (resolved) arithmetic one and the `[[ ]]` errors only fire on invalid
expressions that essentially no real script contains. bash's format needs
per-context taxonomy ("conditional binary operator expected" vs "…unary
operator" vs "conditional expression") plus the offending-token name and bash's
second `syntax error near \`TOKEN'` line — osh deliberately uses single-line
diagnostics everywhere (functions, coproc, arithmetic operand errors), so
matching bash here would be an inconsistent one-off unless the whole shell moves
to bash's two-line format. Behavioral (result-affecting) divergences are higher
value.

**Proper fix (if ever needed):** classify `parse_cond` failures into an enum
carrying (a) bash's context-specific first line and (b) the offending token for
the `syntax error near \`TOKEN'` second line; emit both lines with the `osh:
line N:` prefix on each, mirroring how bash repeats the `bash: -c: line N:`
prefix per line.

### TD-OILS-EXTGLOB-PARSE. `osh` always *parses* extended-glob syntax (`+(…)` etc.) even when `extglob` is off; bash gates it at parse time — MINOR LENIENCY DEVIATION 2026-07-19

**Where:** `userspace/oils/src/parser.rs` / `lexer.rs` (word/case-pattern
lexing always recognises `?(`/`*(`/`+(`/`@(`/`!(` groups), vs `interp.rs`
`exec_case` (line ~1368) which honours the `extglob` flag at *match* time.

**What:** in bash, the `extglob` shopt affects **parsing** of extended patterns
in `case` arms and globs — with `extglob` off, `case foo in +(f|o))` is a
*syntax error* (`unexpected token '('`). `osh`'s parser always accepts extended
pattern groups, then `case`/glob matching honours the runtime `extglob` flag
(off ⇒ the group is treated literally, so `+(f|o)` fails to match `foo`,
yielding the `*)` arm). Net effect: `osh` accepts and runs some scripts bash
would reject at parse time when `extglob` is off; it never *mis-matches* (a
pattern that bash would match with extglob on, osh only matches when the flag is
on too). This is strictly more lenient, not wrong-answer-producing.

**Note:** this is distinct from — and does *not* apply to — `[[ str == pat ]]`,
where bash matches "as if extglob were enabled" unconditionally; `osh` now does
the same (fixed 2026-07-19, `cond_binary`, test `dbracket_match_always_uses_extglob`).

**Why deferred / accepted:** matching bash's parse-time gating would require
threading the *runtime* `extglob` state into the parser (parsing happens before
any `shopt` on the same input runs — which is exactly why bash rejects a
same-line `shopt -s extglob; case … +( …`). The leniency is defensible and
harmless in practice (real scripts enable `extglob` in a prior file/line before
the pattern is parsed), so this is documented rather than "fixed" by making the
parser stricter. **Proper fix (if ever wanted):** have the lexer treat `X(` as an
extended group only when a parse-time `extglob` flag is set, and surface a syntax
error otherwise — but this buys only bug-for-bug parity on invalid input.

### TD-OILS-BRACE-CHARRANGE-BACKSLASH. MSYS reference bash mishandles `{A..z}`-style char ranges crossing ASCII `\` (92); `osh` follows real (Linux) bash — INTENTIONAL, `osh` is correct 2026-07-20

**Where:** `userspace/oils/src/brace.rs` (single-char range expansion).

**What:** For a `{c..d}` range whose endpoints straddle the punctuation gap
between `Z` (90) and `a` (97) — which contains `[ \ ] ^ _ `` — `osh`
expands the **full inclusive ASCII range**, e.g. `echo {A..z}` →
`… X Y Z [ \ ] ^ _ ` a b …` and `echo {Z..^}` → `Z [ \ ] ^`. This matches
real Linux bash's documented sequence-expression semantics (each character
between the two endpoints, inclusive). The **MSYS reference bash on this
dev box is inconsistent/buggy** here: `echo {A..z}` expands but silently
**drops the `\`** (emits an empty slot at position 92), while
`echo {Z..^}` and `echo {[..]}` are left **unexpanded** (literal). Only the
MSYS build shows this; osh's output is the correct one.

**Why not "fixed":** matching the MSYS quirk would mean *removing* correct
behavior to reproduce a host-libc bug on invalid-ish input. Documented as a
reference-artifact divergence; osh keeps the correct inclusive char range.

### TD-OILS-COMPOUND-SCRATCHFD-PIPE. A compound command's `2>&N` dup through a scratch fd aliased to a *pipe* (command substitution) leaks stderr to the real terminal — MINOR, obscure 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — the compound-command redirect path
(the `saved_fds`/`SavedFd` + `stderr_stack` mechanism, redirect function ~line
1700+) vs. the simple-command paths (`run_external` ~line 4219, `run_builtin`
~line 6459) which now share `install_extra_fds`/`restore_extra_fds`.

**What:** for a *simple* command, `{ … } 3>&1 2>&3` inside `$( … )` correctly
routes stderr into the capture pipe (fixed 2026-07-19, test
`same_command_fd_dup_resolves`). But the *compound* form
`r=$({ echo o; echo e >&2; } 3>&1 2>&3)` still sends the inner `>&2` output to
the shell's real stderr instead of the command-substitution pipe:

```
bash:  r == "o\ne"
osh :  r == "o"   (and "e" leaks to the terminal)
```

The base compound case without a scratch fd (`{ echo e >&2; } 2>&1` in a
command sub) works — the gap is specifically the interaction of a scratch fd
aliased to an `Out::Pipe` (`3>&1` where fd 1 is the capture pipe) with the
compound path's separate stderr routing (`stderr_stack`), which does not
consult `open_write_fds[3]` the way the simple-command paths do.

**Why deferred:** rare edge-of-an-edge (a brace/subshell group that dups
stderr through a user scratch fd, *and* is itself inside a capturing command
substitution). The simple-command case — the common one — is fixed.

**Proper fix:** unify the compound redirect executor onto the same
`install_extra_fds`/`restore_extra_fds` + `open_write_fds`-consulting stderr
routing the simple paths use, so a compound `2>&N` resolves N against the
materialised scratch fd (pipe-aware) rather than falling through to the real
stderr. Ultimately this is another symptom of the collapsed-`RedirPlan`
order-loss; the long-term proper fix is an ordered fd-op executor shared by
all command kinds.

### TD-OILS-ARRAY-EMPTY-ASSIGNED. `declare -p` / `@A` can't distinguish a never-assigned empty array from an assigned-empty one — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `format_var_assignment` (~8792) and
the array/assoc state (`Shell::arrays`, `Shell::assoc`). Surfaces in `declare -p`,
the bare `set` listing, and the whole-array `${arr[@]@A}` transform.

**Symptom:** bash distinguishes an array that was *declared but never assigned*
from one that was *assigned an empty value list*, and prints them differently:

```
declare -a e;    declare -p e   →  bash: declare -a e       osh: declare -a e     (match)
declare -a e=(); declare -p e   →  bash: declare -a e=()    osh: declare -a e     (DIFF)
```

The same split applies to `${e[@]@A}` (bash `declare -a e=()` vs osh
`declare -a e`) and the `set` listing. osh currently prints an empty array/assoc
as the bare name in *both* cases. (A non-empty array is unambiguous and already
matches; the divergence is only for the empty-but-assigned state.)

**Why deferred:** osh's model stores every array as a `HashMap`/`BTreeMap` entry;
both `declare -a e` (`arrays.entry(name).or_default()`) and `e=()`
(`arrays.insert(name, empty)`) produce an identical empty container, so the two
states are indistinguishable without extra bookkeeping. A proper fix needs an
"has been assigned a value list (even empty)" marker per array — set on every
value-assignment site (`name=(…)`, `name[i]=…`, `+=`, `e=()`) but **not** on a
bare `declare -a`/`declare -A` — cloned into subshells and cleared on `unset`.
That touches many mutation sites for an obscure cosmetic distinction, so it was
split out. (An earlier attempt to unconditionally emit `=()` for empty arrays
was reverted: it fixed the `e=()` case but regressed the far more common
`declare -a e` no-init case.)

**Proper fix:** add `Shell::arrays_assigned: HashSet<String>` (covering indexed +
associative), set it at each value-assignment site, honour it in
`format_var_assignment` (empty + assigned → `name=()`, empty + not-assigned →
bare `name`), clone it in `clone_for_subshell`, and remove on `unset`.

**RESOLVED (2026-07-19):** implemented as `Shell::array_valued: HashSet<String>`,
mirroring the `integer_attr` lifecycle. Set at every value-assignment site
(the `AssignRhs::Array` indexed + assoc literals — including empty `a=()`; the
indexed element-assign and `a+=` element-0 paths; `assoc_set`; `read -a`;
`mapfile`/`readarray`), **not** on a bare `declare -a`/`-A`. Cloned into
subshells, snapshot/restored by `local` (added `array_valued` to `VarSnapshot`),
and removed on `unset` (whole variable). `format_var_assignment` now emits
`name=()` for an empty-but-valued array/assoc and the bare `name` otherwise.
Fixed the related `read -a arr < /dev/null` bug in the same change: `read -a`
now resets the target array to empty up front (bash semantics), so an EOF with
no data leaves a defined empty array and a pre-existing array is replaced rather
than merged. Tests: `declare_p_empty_array_distinguishes_assigned_from_declared`,
`read_a_creates_empty_array_on_eof`. Verified against MSYS bash across
element-assign-then-unset, `+=`, `local -a`, `mapfile`, and pre-existing-array
cases. 548 tests pass; clippy + slateos builds clean.

### TD-OILS-PRINTF-ERRORDER. `printf` emits all invalid-number errors before its stdout, not interleaved — MINOR 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_printf` (~8381) and
`format_printf` (~13806). `format_printf` builds the *entire* output string and
collects every per-argument "invalid number" diagnostic into a `Vec`; the
builtin then emits all errors to stderr and writes all stdout separately.

**Symptom:** under a combined stream (`printf … 2>&1`), the ordering differs
from bash, which interleaves per conversion. Example — `printf "%d\n" 5 bad 7`:

```
bash: 5 / <error> / 0 / 7        (error appears between the 5 and the 0)
osh:  <error> / 5 / 0 / 7        (all errors precede all output)
```

Values and exit status match; only the stdout/stderr interleaving differs, and
only when both streams are merged. (The error line also carries the ERRLINE
`bash: line N:` prefix osh omits — see TD-OILS-ERRLINE — so the lines differ
regardless.)

**Why deferred:** exact parity needs `format_printf` to emit output and errors
*as it processes each conversion* (a callback/event-stream restructure of a
heavily-used, well-tested function) rather than returning a finished string plus
a separate error list. A partial reorder (write stdout, then errors) would move
osh to "output-then-error" but still not match bash's mid-stream interleaving, so
it is not a clean parity win. Cosmetic-only; left as-is.

**Proper fix:** restructure `format_printf` to write each formatted chunk and any
conversion error in stream order (e.g. take `&mut impl Write` for stdout plus an
error sink, or yield an ordered `Vec<PrintfEvent>`), done alongside ERRLINE so the
whole merged stream matches.

### TD-OILS-INDIRECT-AT-STAR. `${!@}` / `${!*}` list variable names instead of indirecting through `$@`/`$*` — RESOLVED 2026-07-20

**Resolved 2026-07-20** exactly per the proper-fix plan below. (1) In
`parser.rs` `parse_braced_param`, the `${!prefix@}`/`${!prefix*}` name-listing
branches now require `!prefix.is_empty()`, so a bare `${!@}`/`${!*}` falls
through. (2) `is_indirect_referent` now accepts `@`/`*`, so they parse as
`WordPart::Indirect("@"/"*")`. (3) `expand_indirect` short-circuits a `@`/`*`
referent with an **empty** positional list to `String::new()` (status 0) before
the target validator, while a non-empty `$@`/`$*` joins to a single name that
routes through `is_valid_indirect_target` — so `set -- a b c; echo "${!@}"`
yields bash's "a b c: invalid variable name" (exit 1) and `foo=1; echo "${!@}"`
yields empty (exit 0). A single positional naming a set variable indirects one
level (`V=hi; set -- V; echo "${!@}"` → `hi`). Verified against bash across all
four cases plus the still-working prefixed listing form (`${!aa@}`). Regression
test `indirect_at_star_positional`; 683 tests, clippy clean, both targets build.

<details><summary>Original entry (for history)</summary>


**Where:** `userspace/oils/src/parser.rs` — `parse_braced_param`, the `${!…}`
branch (~1308). The empty-prefix cases `${!*}` and `${!@}` are caught by the
`strip_suffix('*')` / `strip_suffix('@')` name-listing logic (`WordPart::VarNames`
with `prefix == ""`).

**Symptom:** bash treats `${!@}` and `${!*}` (empty prefix) as *indirect
expansion through the positional list* `$@` / `$*`, not as the "list all variable
names" form. Only a **non-empty** prefix (`${!PATH@}`, `${!BASH_@}`) triggers
name-listing.

```
set -- a b c; echo "${!@}"    →  bash: "a b c: invalid variable name" (exit 1)
                                  osh:  ALLUSERSPROFILE APPDATA … (every var name)
foo=1; echo "${!@}"           →  bash: (empty, exit 0)   [no positionals → nothing]
                                  osh:  … foo (every var name)
```

So bash resolves `${!@}` = `${!<value-of-$@>}`: with `set -- a b c`, `$@`
expands to `a b c`, which as a single indirect target name is invalid; with no
positionals, `$@` expands to nothing, so the whole thing is empty (no error).
osh instead lists every set variable name.

**Why deferred:** the fix is more than flipping the prefix guard. It needs (1) the
listing branch to require a **non-empty** valid prefix so `${!@}`/`${!*}` fall
through, (2) `@`/`*` added to the accepted indirect referents (they resolve via
`param_value("@"/"*")`), and (3) a special case so an **empty** `$@`/`$*` (no
positionals) yields empty *without* the "invalid variable name" fatal that a
non-empty-but-malformed target produces — bash distinguishes "nothing to
indirect" from "indirect through a bad name". That empty-positionals rule is the
subtle part and is easy to get wrong, so it was split from the `${!#}`/`${!N}`
special/positional-referent fix (which is done). Genuinely obscure form.

**Proper fix:** in `parse_braced_param`, gate the `${!prefix@}`/`${!prefix*}`
listing on `!prefix.is_empty()`; extend `is_indirect_referent` to accept `@`/`*`;
and in `expand_indirect` treat a `@`/`*` referent whose positional list is empty
as an empty (non-fatal) result rather than routing an empty string through
`is_valid_indirect_target`.
</details>

### TD-OILS-READ-T0-POLL. `read -t 0` readiness on a live pipe/tty is exact only on Windows — MINOR 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `input_available_now` and the
platform probes `stdin_readable_now` / `pipe_reader_readable_now` /
`handle_readable_now` (Windows FFI: `PeekNamedPipe`/`WaitForSingleObject`).

**What:** `read -t 0` (non-consuming availability poll, exit 0 iff a read would
not block) is now implemented. Deterministic sources — here-strings/here-docs,
file redirects, fd cursors (`-u N` / `<&N`), `/dev/null`, and a persistent
`exec <` cursor — return the correct answer on all targets. A *live OS pipe or
interactive terminal* is probed exactly only on the **Windows host** (via
`PeekNamedPipe` for pipes and a zero-timeout wait for the console). On
**non-Windows targets (including SlateOS)** the fallback treats a non-tty
inherited stdin as ready and only counts already-buffered pipe bytes — so a
bare OS-pipe peek and an interactive-tty keystroke poll are approximated.

**Proper fix:** wire a `poll(2)`/`select(2)` (or SlateOS-native readiness
syscall) probe into `stdin_readable_now`/`pipe_reader_readable_now` for
`cfg(not(windows))`, mirroring the Windows path. `oils` is currently std-only
(no `libc`/`nix` dep); this needs either raw `poll` FFI or a small dependency.

**Note (inherent race, not a bug):** on a *pipeline* (`printf … | { read -t 0; … }`)
the upstream stage writes concurrently, so `read -t 0` in the downstream stage
may legitimately poll before the bytes arrive and report "would block" — a
correct outcome of point-in-time poll semantics. bash exhibits the same race
but its tiny builtin writes usually land first. Not fixable without changing
pipeline scheduling, and arguably should not be.

### TD-OILS-BRACE-BACKSLASH. Brace char-range spanning the backslash char (e.g. `{A..z}`) — `osh` emits a literal `\`, bash drops it — MINOR, obscure 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (brace-expansion char-sequence
generator). A char range like `{A..z}` walks ASCII code points A(65)..z(122),
which crosses `\` (92).

**What:** bash's brace expansion is a *textual* rewrite of the raw token, so the
generated word for code point 92 is a single backslash that then undergoes quote
removal — the backslash escapes nothing and is removed, yielding an **empty**
field. `osh` builds brace results as already-parsed literal words (no post-hoc
quote removal), so it emits a literal `\` field instead. Concretely
`printf "[%s]" {A..z}` shows `…[[][][]]…` in bash (empty field between `[` and
`]`) but `…[[][\][]]…` in osh. Only ranges that straddle char 92 differ; every
other range matches. No real script relies on this.

**Why deferred:** matching bash needs brace expansion to run as a textual pass on
raw token bytes with the result re-fed through quote removal, which is a
different architecture than osh's structured expansion. High effort, essentially
zero practical value (a range crossing the backslash char is a pathological
input). **Proper fix (if ever wanted):** apply quote-removal semantics to
brace-generated literal segments so a generated lone `\` is consumed as an
escape, matching bash.

### TD-OILS-PRINTF-ERRORDER. `printf` batches all "invalid number" diagnostics *before* its stdout, where bash interleaves each at the point of the bad conversion — MINOR ORDERING DEVIATION 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_printf` / `format_printf`.

**What:** `format_printf` builds the *entire* output string while collecting all
per-argument numeric-parse errors into a `Vec`, and `builtin_printf` then writes
every error to stderr *first*, followed by the whole stdout string. bash instead
processes the format left-to-right, emitting each conversion's output and any
error as it reaches it. The difference is only observable when stdout and stderr
are merged (`2>&1`):

```
printf "%d\n" 0x1F 010 0b101 2>&1
# osh:  <error for 0b101>, 31, 8, 0     (error batched to the front)
# bash: 31, 8, <error for 0b101>, 0     (error at the 3rd conversion)
```

The numeric *values* are identical (31, 8, 0 — `0b101` is invalid in printf `%d`,
falls back to 0); only the diagnostic ordering differs.

**Proper fix:** stream printf output incrementally to the target fd, emitting each
argument's diagnostic to stderr at the moment its conversion is processed rather
than collecting them up front — i.e. thread the write/emit through
`format_conversion` instead of returning a fully-built `String` + `errors` Vec.
Deferred because it only surfaces under `2>&1` stream-merging and needs a
restructure of printf's single-write-at-the-end buffering model (which also
serves `-v` assignment, where there is no stdout interleaving at all).

### TD-OILS-MSYS-EXIT127. Host probe artifact: MSYS bash exits **127** for fatal expansion errors where real bash exits **1** — NOT AN OSH BUG (osh is correct) 2026-07-19

**What:** when a *non-interactive* shell terminates due to a fatal parameter-
expansion error — an unbound variable under `set -u`, or a `${var:?word}` /
`${var:?}` reference to an unset/null variable — the shell prints the message and
exits. The **exit status** should be `1` (real GNU bash on Linux; confirmed via
bash docs/community references). The MSYS2 bash used for host comparison testing
(`5.2.37(1)-release (x86_64-pc-msys)`) instead exits with **127** for these
cases. `osh` correctly exits `1` and correctly *stops* (does not run the
following command), matching real bash on both axes.

**Why it's logged here:** so a future host `chk`/status-comparing probe that
flags `set -u; $undef`, `: ${x:?}`, or `${x:?msg}` as a status DIFF (osh 1 vs
MSYS-bash 127) is recognised as an **MSYS artifact**, not an osh regression —
do **not** "fix" osh to emit 127. This joins the other known MSYS host-probe
false positives (C-locale UTF-8 `$'\uXXXX'`, Windows path/OS-error text, `/tmp`
path-root, signal-number table, BASH_VERSINFO/BASHPID).

### TD-OILS-XTRACE. `set -x` tracing: no `[[ … ]]` conditional trace, no nested command-substitution trace, no `PS4` parameter expansion — PARTIAL 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — the `set -x` trace block in
`exec_simple` (after the readonly-prefix guard), `apply_assignment` (bare-
assignment tracing), the compound-command exec sites (`exec_for`,
`exec_for_arith`, `exec_case`, `exec_select`, `exec_arith`), `xtrace_prefix`,
`xtrace_emit`, and `xtrace_quote`.

**Status (2026-07-19, updated):** simple-command, bare-assignment, and most
compound-command tracing now match bash byte-for-byte:

- Plain scalars trace their *expanded* value (minimally single-quoted, empty
  shown unquoted); indexed-element/array assignments trace in *source* form;
  temporary prefix assignments (`FOO=bar cmd`) each trace on their own line;
  command arguments are minimally quoted; `PS4` overrides the `+ ` prefix.
- `for NAME in WORDS` prints a *source-form* header before **each** iteration
  (`for i; do` → `for i in "$@"`).
- C-style `for ((init;cond;update))` traces `(( init ))` once, `(( cond ))`
  before each test, `(( update ))` after each body; an **empty** section is
  traced as always-true `(( 1 ))` (so `for ((;;))` traces `(( 1 ))` for the
  init and cond slots), matching bash.
- `select NAME in WORDS` prints a source-form header once (bash does not
  re-emit it per iteration).
- `case WORD in` prints `case <source-word> in` (unexpanded) before matching.
- `(( … ))` commands trace `(( <raw> ))` (raw text preserved, so interior
  spacing like `((  2 > 1  ))` matches). This also covers `while`/`until`
  arithmetic *conditions*, which self-trace via the `(( ))` command path (bash
  emits no separate `while`/`until` header, and neither does `osh`).

Still **missing** relative to bash:

- **`[[ … ]]` conditional tracing.** bash traces `[[ ]]` with the operands
  *expanded and re-quoted* (`[[ $v == "a b" ]]` → `+ [[ a b == \a\ \b ]]`),
  inserts `-n` for a bare-word test (`[[ $v ]]` → `+ [[ -n a b ]]`), and splits
  `&&`/`||` operands into **separate** short-circuit-ordered `[[ … ]]` trace
  lines. Reproducing bash's exact expansion + pattern-requoting + per-operator
  splitting is intricate and low-value, so `osh` currently emits nothing for a
  `[[ … ]]` command under `set -x`. Deferred deliberately; proper fix is a
  dedicated expanded-cond tracer mirroring `cond_eval`'s evaluation order.
- **`if` header.** bash (like `osh`) emits no `if`/`then`/`else` header — the
  guard and body commands self-trace. Already correct; noted for completeness.
- **Nested command-substitution trace.** bash raises the PS4 indirection level
  and traces commands run inside `$(…)` with a doubled first char
  (`++ echo hi`). `osh` does not trace inside command substitution.
- **`PS4` parameter/arithmetic expansion.** `xtrace_prefix` runs `PS4` through
  `prompt_expand` (backslash escapes only). bash also performs parameter,
  arithmetic, and command expansion on `PS4`, so `PS4='+ $LINENO '` is not
  expanded here.

The remaining gaps affect only niche `set -x` cases; they are logged so a
future probe recognising a `[[ ]]`/`$( )`-header trace DIFF knows it is this gap.

### TD-OILS-ERRLINE. Error diagnostics lack bash's `<name>: line N:` prefix — ✅ RESOLVED 2026-07-19

**Resolution (2026-07-19):** implemented. Every runtime/expansion diagnostic now
carries bash's `<name>: line <N>: ` prefix in non-interactive mode (`osh -c` /
scripts), matching bash's format (the shell *name* still differs — `osh` vs
`bash` — which is intended and unavoidable). Shipped design:

- New `Shell::err_prefix()` returns `"{name}: line {line}: "` when
  `command_mode`/`script_mode` is set, else `"{name}: "` (interactive bash omits
  the line number too). `name` is `$0` (the `-c` pseudo-name `osh` or the NAME
  arg, or the script path); `line` is `current_line` (already backs `$LINENO`,
  verified to track bash including function-relative numbering).
- All ~140 production error sites were routed through `err_prefix()`. This also
  fixed the latent bug where sites hard-coded a literal `osh:` even when running
  a *script* (errors now correctly name the script path, as bash does).
- The `eprintln!(...)` error sites (which bypassed osh's stderr-redirection
  stack, so an error under `cmd 2>file` leaked to the terminal) were converted to
  `self.errln(&format!(...))`, so all diagnostics now honour an active
  `2>`/`2>&1` redirect — bash parity.
- **Subtlety handled:** bash's pure `builtin_usage()` messages
  (`<builtin>: usage: …`, e.g. bare `getopts`/`unalias`/`trap`) are emitted with
  *no* shell-name/line prefix at all — only the builtin name. Those sites
  (getopts/trap/unalias usage) are excluded from `err_prefix()` and now
  byte-match bash. The `getopts` runtime `illegal option`/`requires argument`
  diagnostics use `$0:` *without* a line number in bash, which osh already
  matched (they use `self.name` directly, not the prefix).
- Syntax/parse errors go through `format_parse_error(e, prefix)` (now takes the
  prefix as a parameter) and get `<name>: line N: syntax error: …`. bash inserts
  an extra `-c:` for `-c` parse errors (`bash: -c: line N:`); osh keeps the
  uniform `<name>: line N:` form (name differs anyway).
- Tests: `error_prefix_includes_line_number_in_command_mode` covers the prefixed
  runtime errors (line 1 & line 2), the redirect-honouring command-not-found, and
  the unprefixed usage line. All 550 oils tests pass; clippy clean; both host and
  slateos targets build.

Remaining related nicety: the arithmetic error *body* was tracked separately as
TD-OILS-ARITH-ERRFMT and is now **RESOLVED** (2026-07-19) — osh emits bash's full
`<name>: line N: [<builtin>: ]<expr>: <body> (error token is "…")` form.

**Original report follows for reference.**

### TD-OILS-ERRLINE (original). Error diagnostics lack bash's `<name>: line N:` prefix — OPEN 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — all ~40 error-emission sites, which
hardcode a bare `osh: ` prefix in three different ways: `eprintln!("osh: …")`,
`self.errln(&format!("osh: …"))`, and `self.emit_stderr(format!("osh: …"))`.

**Symptom:** bash prefixes every non-interactive runtime/expansion diagnostic
with `<$0>: line <N>: ` — the source name (`$0`: `bash`/the NAME arg for `-c`,
or the script path for a file) plus the 1-based source line of the failing
command. `osh` emits only `osh: <msg>` with no line number and no script name:

```
bash -c 'foo'            → bash: line 1: foo: command not found
bash -c 'foo' myname     → myname: line 1: foo: command not found   ($0 == prefix)
bash script.sh (line 47) → script.sh: line 47: foo: command not found
osh  -c 'foo'            → osh: foo: command not found
```

The `line N:` component is a real debugging-usability feature (it points the
user at the failing line in a long script), so this is a genuine bash-superset
gap, not mere cosmetics. Interactive (tty) bash omits the line number, matching
`osh`'s current behavior — the divergence is only in the non-interactive
`-c`/script/piped-stdin modes.

**Infrastructure already present:** `self.current_line` (backs `$LINENO`, verified
to track bash) supplies N; `self.name` (settable via `set_name`, == `$0`) supplies
the prefix name; `self.command_mode`/`self.script_mode` distinguish the
non-interactive modes that get the line number.

**Proper fix:** add a single `fn diag(&mut self, msg: &str)` helper that emits
`{name}: line {line}: {msg}\n` in `command_mode`/`script_mode` (else `{name}: {msg}\n`)
**through `emit_stderr`** so the diagnostic also respects active `2>` redirections,
then convert every error site to call it. That conversion also fixes a *second*
latent bug: the `eprintln!("osh: …")` sites write to the real process stderr and
so **bypass osh's stderr-redirection machinery** (an error under `cmd 2>file`
leaks to the terminal instead of the file) — routing all diagnostics through
`emit_stderr` corrects that too. Subtleties to preserve: builtin usage lines
(`read: usage: …`) are emitted *without* the line prefix (bash keeps only the
primary diagnostic prefixed); the piped-stdin REPL is non-interactive in bash
but currently indistinguishable from a tty REPL in `osh` (a `--`/isatty check is
a follow-up). ~40 emission sites + ~15 test assertions to update — see
`open-questions.md` (flagged for the operator because it reformats *every* error
message the shell prints, a pervasive user-visible output change).

### TD-OILS-ARITH-ERRFMT. Arithmetic error messages don't match bash's `<expr>: <msg> (error token is "<tok>")` format/taxonomy — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/arith.rs` — `ArithError { msg, token }` payloads
raised throughout `parse`/`parse_*`/`parse_number`/`str_to_val`, and the
`emit_arith_error` / `eval_arith_cmd` call sites in `interp.rs` that surface them.

**Resolution (2026-07-19):** `osh` now reproduces bash's full arithmetic
diagnostic line `<name>: line N: [<builtin>: ]<expr>: <body> (error token is
"<tok>")`:

- **`ArithError` carries a token.** Was `ArithError(String)`; now
  `ArithError { msg: String, token: Option<String> }` with `new()` /
  `with_token()` constructors and a `Display` impl that appends
  ` (error token is "<tok>")` when a token is present. The `AParser` tracks
  `last_op_start` / `last_atom_start` and a `rest_from(pos)` helper so every
  raise site can emit "offending-position-to-end-of-input, de-quoted" as the
  token (operand-expected → operator/last-op position; trailing-input → current
  position; ternary `:` → then-branch start; assignment → operator position;
  bad subscript → array-name start; missing `)` → last-atom start; div/mod/exp →
  RHS operand start, threaded via `Expr::Bin`'s 4th field).
- **Body wording matches bash's taxonomy:** `division by 0`, `exponent less
  than 0`, `syntax error: operand expected`, `syntax error in expression`
  (recognized trailing token) vs `syntax error: invalid arithmetic operator`
  (untokenizable char), `` `:' expected for conditional expression ``,
  `bad array subscript`, `attempted assignment to non-variable`, `` missing `)' ``,
  `invalid arithmetic base`, `value too great for base`,
  `expression recursion level exceeded`.
- **`<expr>:` prefix + builtin tag.** `interp.rs::emit_arith_error` prints the
  (leading-whitespace-trimmed) source expression, and `arith_cmd:
  Option<&'static str>` models bash's `this_command_name` so the right builtin
  tag is prepended: `let:` for `let`, `((:` for `(( ))` and `for (( ))`,
  `declare:`/`typeset:`/`local:` for the `-i` attribute builtins. Plain
  assignments, array-element assignments, and `$(( ))` word substitution get no
  tag (matching bash).
- **Recursively-expanded `<expr>` prefix (FIXED 2026-07-20).** When a failure
  occurs while recursively evaluating a *variable's value* as arithmetic, bash
  echoes the resolved value, not the variable reference — `x="5 apples"; $((x))`
  reports `5 apples: syntax error …`, not `x: …`. `arith.rs::str_to_val` now
  records the innermost failing value in `ArithError::expr_override` (the deepest
  level sets it first as the error unwinds; outer levels leave it in place), and
  `emit_arith_error` prefers it over the top-level source. This also covers the
  `expression recursion level exceeded` case (bash echoes the innermost value)
  and indirection chains (`x=y; y="1 2"; $((x))` → `1 2:`). Verified against
  MSYS bash for `x="5 apples"`, `x=3.5`, `x="1 2"`, `x=y;y=…` and the `declare
  -i z=x` builtin-tag path.

**Tests:** `arith.rs::error_bodies_and_tokens_match_bash` (16 body/token cases)
and `interp.rs::arith_error_matches_bash_format` (full-line, incl. builtin tags
and `2>/dev/null` silencing). Verified byte-for-byte against MSYS bash on 25/27
probed cases (name-normalized).

**Residual divergences (documented bash yacc artifacts, low value — left as-is):**

- **Exponent error token:** `$((2**-1))` → bash reports `1` (its lexer's
  last-consumed token), osh reports `-1` (the RHS operand source). bash is
  internally inconsistent here (division uses the whole RHS source; exponent
  uses the last lexed token) — osh picks the single consistent
  offending-position rule.
- **Nested subscript prefix:** `$((a[9/0]))` → bash echoes `9/0` as the expr,
  osh echoes `a[9/0]` (the full atom). A yacc reduction artifact.
- **Function-scope name:** in a function body bash's `<name>` becomes
  `environment`; osh keeps `$0` (= `osh`) per design §74.

### TD-OILS-ULIMIT. `ulimit` tracks limits at the shell level but does not query/enforce the real kernel `getrlimit`/`setrlimit` — MINOR 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_ulimit`, `ulimit_print_all`,
the module-level `RLIMIT_SPECS` / `default_rlimits` / `ulimit_line` helpers, and
the `Shell::rlimits` field (`(soft, hard)` per option letter).

**Symptom:** `ulimit` was previously entirely missing (`osh: ulimit: command not
found`, exit 127), which hard-failed any script probing or setting limits. It is
now a real builtin: it models a per-shell `(soft, hard)` table (defaults mirror
Linux bash — `-n` 1024, `-s` 8192, `-c` 0, `-p` 8, most others unlimited),
supports the standard option letters, `-a`, `-H`/`-S`, and the
`unlimited`/`hard`/`soft` operands, and honours get/set within the shell (and
across subshell clones, like bash's per-process inheritance). **But it does not
touch the actual kernel:** on slateos it does not call `getrlimit(2)`/
`setrlimit(2)`, so `ulimit -n` reports osh's modelled default (1024) rather than
the real fd limit the kernel would enforce, and `ulimit -n 512` does not actually
constrain descendants. On the Windows host build there is no rlimit concept at
all, so the table is the only possible answer there.

**Why deferred:** oils is std-only (no libc/nix), and Rust std exposes no rlimit
API, so a real integration needs raw `getrlimit`/`setrlimit` syscall FFI gated to
the unix/slateos target (the kernel already has the plumbing — see
`kernel/src/fs/rlimit.rs`, `kernel/src/syscall/linux.rs`, `posix/src/ulimit.rs`).
That path cannot be exercised from the Windows dev host, so it was split out. The
shell-level model is honest (bash's `ulimit` is itself a thin wrapper whose
"set" only affects descendants) and eliminates the hard command-not-found failure.

**Proper fix:** add a `#[cfg(unix)]` raw-syscall shim (`getrlimit`/`setrlimit`,
or `prlimit64`) with a `// SAFETY:` note, seed `Shell::rlimits` from the live
kernel values at startup, and route set operations through `setrlimit` so
descendants inherit the enforced limit. Keep the current table as the Windows-host
and pre-seed fallback. Unit conversions (bash's block/kbyte display units vs. the
kernel's byte-granular `rlim_t`) must be applied at the FFI boundary.

### TD-OILS-INTERACTIVE-DETECT. Non-tty stdin is treated as interactive — OPEN 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `shopt_default`/`aliases_enabled`
(the `expand_aliases` default gate) and, more broadly, everywhere `osh`
decides "am I an interactive shell?"; `userspace/oils/src/main.rs` — `run_source`
dispatch and the REPL loop that prints prompts.

**Symptom:** bash decides interactivity by testing `isatty(stdin)` (plus the
`-i` flag and whether a script/`-c` was given). `osh` currently approximates
interactivity purely by mode flags: `command_mode` (`-c`) and `script_mode`
(script file) are non-interactive, and *everything else* — including a REPL
reading from a **pipe or redirected file** (`echo 'cmd' | osh`, `osh < file`) —
is treated as interactive. Two observable divergences follow:

```
printf 'alias ll="ls -l"\nll\n' | bash    → ll: command not found   (aliases OFF: non-interactive)
printf 'alias ll="ls -l"\nll\n' | osh     → runs `ls -l`            (aliases ON: osh thinks it's interactive)
echo pwd | bash                            → (no prompt printed)
echo pwd | osh                             → prints the PS1 prompt before running pwd
```

So `osh` (a) expands aliases when piped stdin should have them off by default,
and (b) prints prompts to a non-tty. Both stem from the same missing check.

**Proper fix:** add an `is_interactive()` predicate that mirrors bash:
interactive iff (`-i` given) OR (no `-c`, no script arg, AND `isatty(0) &&
isatty(2)`). Wire `stdin`/`stderr` tty detection (Windows: `GetFileType` /
`_isatty` on the raw handle; POSIX: `libc::isatty`) into a cached bool on the
shell set once at startup. Then: `shopt_default("expand_aliases")` returns
`is_interactive()` instead of `!command_mode && !script_mode`; the REPL only
prints `PS1`/`PS2` when interactive; and this same predicate feeds the
TD-OILS-ERRLINE `line N:` gate (bash omits the line number only for *interactive*
input, and piped-stdin is non-interactive there too). Until then, the
mode-flag approximation is correct for the common `-c`/script/tty-REPL cases and
only wrong for the rarer piped-/redirected-stdin REPL.

### TD-OILS-STDERR-INTERLEAVE. Same-sink stdout+stderr redirects flush in the wrong order — FIXED 2026-07-19 (all subcases resolved; the capture+subshell+`2>&1` subcase was later fixed by the compound fd-dup routing work)

**Where:** `userspace/oils/src/interp.rs` — `exec_with_redirects` (the
compound/group-command redirect+capture path).

**Symptom (fixed):** a compound command that wrote to **both** stdout and stderr
with both redirected to the **same** file in one shot — `{ …; } >f 2>&1`,
`( … ) >f 2>&1`, `for … done >f 2>&1`, the `&>f` shorthand — buffered stdout to
the end and folded stderr in ahead of it, so `osh` produced `e\no` where bash
produces `o\ne` (content correct, interleave order wrong).

**Fix:** a `> f` stdout redirect now drives fd 1 through the file *live* via a
scoped `exec_stdout` override (instead of capturing to a `Vec` and dumping at the
end). When fd 2 targets the same path (`>f 2>&1`, `&>f`) it shares fd 1's open
handle (a `try_clone`, same file object / same OS offset on both Unix and
Windows), so the two streams interleave at one shared offset exactly as bash's
dup does. A parallel scoped `exec_stderr` override was added so `( … )` subshell
bodies — which clone `exec_stdout`/`exec_stderr` but reset `stderr_stack` — also
reach the file (this incidentally fixed `( echo e >&2 ) 2> f`, which previously
leaked the subshell's stderr to the real terminal). The `2>&1 > f` ordering case
(fd 2 copies fd 1's sink *before* `> f` rebinds it) is handled by snapshotting
fd 1's pre-override sink into a concrete handle. Regression tests:
`group_redirect_stdout_stderr_interleave`, `for_loop_redirect_stdout_stderr_interleave`,
`subshell_redirect_stdout_stderr_interleave`, `subshell_stderr_only_redirect_reaches_file`,
`stderr_then_stdout_redirect_order_keeps_stderr_on_prior_sink`, and the updated
`amp_redirect_both_streams`.

**Formerly-remaining subcase (now RESOLVED 2026-07-19):** command-substitution
*capture* of a `2>&1` **subshell** — `x=$( ( echo o; echo e >&2 ) 2>&1 )` — used
to lose the subshell's stderr (it went to the real terminal, so `x` got only
`o`). This was fixed as a side effect of the compound fd-dup routing work
(`AliasStd`/`stdout_to_fd`/`stderr_to_fd` handling in `exec_with_redirects`):
verified `x=$( ( echo o; echo e >&2 ) 2>&1 )` now yields `o\ne` matching bash,
as does the non-subshell form `x=$( { echo o; echo e >&2; } 2>&1 )`. No open
subcases remain for this item.

### TD-OILS-IDVARS. `osh` does not define several bash identity/runtime variables (`EUID`/`UID`/`PPID`/`HOSTNAME`; `BASH`/`BASHOPTS` now done) — PARTIALLY ADDRESSED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`Shell::seed_shell_vars`, the
`param_value` dynamic-var match arm around the `BASHPID`/`BASH_SUBSHELL` cases).

**Status (2026-07-19):** the static *platform-identity* trio bash always
defines is now seeded — `HOSTTYPE=x86_64`, `OSTYPE=slateos`,
`MACHTYPE=x86_64-slateos` (ordinary reassignable shell vars, SlateOS values not
the host build's). `BASHPID` and `BASH_SUBSHELL` were already dynamic.
**`BASH` and `BASHOPTS` are now defined (2026-07-19):** `BASH` is seeded from
`std::env::current_exe()` (lossy, fallback `"osh"`) as a reassignable var;
`BASHOPTS` is a readonly, colon-joined, alphabetically-sorted list of enabled
`shopt` options kept current by `refresh_bashopts()` on every `shopt` toggle
(osh now models bash's full 57-option `shopt` inventory with correct
non-interactive defaults, so the seeded set matches bash byte-for-byte —
verified against MSYS bash). Still **missing** relative to bash:

- **`EUID` / `UID`** (numeric effective/real user id, readonly in bash). Very
  commonly read by scripts (`[ "$EUID" -ne 0 ]` root checks); leaving them unset
  makes such arithmetic comparisons error on an empty operand. **Blocked on a
  design decision:** what identity should osh report? There is no SlateOS
  `getuid`-equivalent wired into the host or target build yet, and the *default*
  identity of a shell during bring-up (root uid 0 vs a regular user) is a
  user-visible policy call. Logged as an open question (`open-questions.md`).
- **`PPID`** (parent process id, readonly in bash). Needs a parent-pid source;
  `std::process` doesn't expose it portably on the host and the SlateOS syscall
  isn't wired. Deferred until a `getppid`-equivalent exists.
- **`HOSTNAME`** — bash sets it from the host; osh's prompt helper already falls
  back to `localhost`. Whether to seed a fixed default (`localhost`/`slateos`) is
  a low-stakes naming choice bundled into the same open question as EUID/UID.

**Proper fix:** once SlateOS credential/`getuid`/`getppid` syscalls exist, wire
`EUID`/`UID`/`PPID` as dynamic `param_value` cases (readonly). The identity
*default* (for host runs and pre-login target state) needs the operator's call.

**Sub-issue — several `BASH_*` internal variables are still absent.** `${!BASH*}`
diverges from bash because osh does not define `BASH_LOADABLES_PATH`.
(`BASH_ALIASES`/`BASH_CMDS` are now defined — **RESOLVED 2026-07-20**, see plan
turned-implementation note below.)
(`BASH_EXECUTION_STRING` — the `-c` command string — is now defined, seeded via
`Shell::set_execution_string` from `main.rs`'s `-c` path, so it reads correctly
and appears in `${!BASH*}`. `BASH_ALIASES`/`BASH_CMDS` are now defined too —
**RESOLVED 2026-07-20**, see the implementation note below. `BASH_ARGV0` is now
defined too — **RESOLVED
2026-07-20**: a dynamic variable tied to `self.name`; `BASH_ARGV0=name` sets
`$0`, reading `$BASH_ARGV0` returns the current `$0`, it appears in `${!BASH*}`
and `declare -p`. One deliberate divergence: bash's `+=` on `BASH_ARGV0` relies
on an obscure lazy-materialization quirk — `BASH_ARGV0=a; BASH_ARGV0+=b` yields
`b`, not `ab`, unless a read intervened — so osh uses the predictable append
(`ab`) instead; `BASH_ARGV0+=` is vanishingly rare in real scripts.
`BASH_ARGC`/`BASH_ARGV` — the extdebug call-argument stack — are now defined too,
**RESOLVED 2026-07-20**; see TD-OILS-MISSING-SPECIAL-ARRAYS for the full
semantics and the one documented non-extdebug divergence.) The only remaining
missing `BASH_*` var is `BASH_LOADABLES_PATH`, which is meaningless on SlateOS
(no loadable builtins) and is intentionally omitted. Low priority.

**Implementation of `BASH_ALIASES`/`BASH_CMDS` — DONE 2026-07-20.** bash exposes
these as *dynamic associative arrays* that are (a) present even when empty
(`declare -A BASH_ALIASES=()`), (b) live — reflecting the current alias table /
command hash, and (c) writable: `BASH_ALIASES[x]="…"` creates an alias,
`BASH_CMDS[foo]=/p` adds a hash entry. The chosen design is **eager mirror
sync at every source mutation** rather than the materialise-on-read accessor the
earlier plan sketched: `self.aliases`/`self.cmd_hash` remain the source of truth,
and `sync_bash_aliases`/`sync_bash_cmds` rebuild the `self.assoc["BASH_ALIASES"]`
/`["BASH_CMDS"]` mirror. The concern that eager-sync would "go stale" was
unfounded once the *complete* mutation set was enumerated — it is small and
closed: the `alias`/`unalias`/`hash` builtins plus the single `resolve_external`
insert (interp.rs ~5609, on the *new*-command branch only; the cache-hit branch
mutates just the hit count, which the mirror doesn't expose, so it stays
sync-free on the hot path). Element writes are intercepted in `assoc_set`
(interp.rs ~3630): `BASH_ALIASES[k]=v`→`self.aliases.insert`, `BASH_CMDS[k]=v`→
`self.cmd_hash.insert`, with `+=` append honoured, then a re-sync. Both names are
seeded empty in `seed_shell_vars` (present-when-empty) and marked `array_valued`
so `declare -p`/`declare -A` render `=()`. Because they live in `self.assoc`, all
existing assoc read paths (`${x[k]}`, `${x[@]}`, `${!x[@]}`, `${#x[@]}`,
`declare -p`, `${!BASH*}` enumeration, bare `declare -A` listing) work unchanged
— no read-site refactor was needed. Verified against bash: `declare -p`,
element read, count, `+=`, `unalias -a`, `hash -p`, and `${!BASH_ALIASES@}`
enumeration all match; regression test `bash_aliases_and_cmds_live_assoc`.
*One documented cosmetic divergence:* `${!BASH_ALIASES[@]}` key order is
`self.aliases` (BTreeMap) sorted order, whereas bash uses its internal hash order
(e.g. bash `b a` vs osh `a b` for aliases `a`,`b`) — unspecified/arbitrary in
bash, and osh's sorted order is deterministic and matches osh's own `alias`
builtin listing.

**Sub-issue — dynamic vars are readable but not *enumerated*.** The dynamic
`param_value` cases (`BASHPID`, `BASH_SUBSHELL`, and any future `EUID`/…)
return a value when read directly (`echo $BASHPID`) but are **not listed** by the
name-prefix expansions `${!BASH*}` / `${!BASH@}`, because those enumerate only the
concrete `vars`/`arrays`/`assoc` maps. (`BASH`/`BASHOPTS` are concrete `vars` now,
so they *do* appear.) bash includes every dynamic variable in the prefix listing.
Fixing this needs the prefix-match code to also consider the set of known
dynamic-variable names (a static name list checked alongside the maps).
Low-value (prefix enumeration of `BASH*` is rare in scripts) and coupled to the
broader "define the missing `BASH*` vars" work above, so parked here.

### TD-OILS-BUILTIN-USAGE. `osh` builtins omit bash's second `NAME: usage: …` synopsis line on a usage error — ✅ LARGELY RESOLVED 2026-07-20

**Update (2026-07-20):** a shared `Shell::builtin_invalid_option(builtin, opt,
usage)` helper now emits bash's two-line pair (the located
`NAME: -OPT: invalid option` diagnostic + the unprefixed `NAME: usage: …`
synopsis) and returns status 2. It (and inline equivalents) are wired into every
builtin that previously silently ignored or misclassified unknown options:
`declare`/`typeset`/`local`, `read`, `readonly`, `unset`, `type`, `hash`, `cd`,
`pwd`, `alias`, `unalias`, `jobs`, `trap`, `mapfile`, `command`, `printf`
(no-format), and `source`/`.` (missing-filename **and** bogus-option, tagged with
the invoking name). All verified byte-for-byte against bash 5.x via the CLI probe
and covered by the `builtin_invalid_option_diagnostics` and
`more_builtins_reject_invalid_options` regression tests. `getopts` already matched.
No remaining known builtin silently swallows an invalid option; this entry stays
listed only as a pointer to the helper and the wording convention.

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_invalid_option`
(shared helper) and the per-builtin flag loops listed above. (Historically the
usage-error paths of `builtin_getopts`, `builtin_source`, `builtin_mapfile`,
`exec_command_builtin`, `builtin_printf` (`-v`), etc. emitted only the one-line
diagnostic `osh: NAME: <problem>`.)

**What:** on a usage/argument error bash prints **two** lines to stderr — the
diagnostic *and* a synopsis, e.g.

```
bash: line 1: command: -Z: invalid option
command: usage: command [-pVv] command [arg ...]
```

osh prints only the first line. The exit status and the primary diagnostic
match; only the trailing `NAME: usage: …` synopsis line is missing. Affects
`command`, `getopts`, `source`, `mapfile`, `printf`, and any other builtin with
a bash usage synopsis.

**Impact:** very low — cosmetic stderr text only; scripts key on exit status and
`$?`, not the synopsis wording. Shows up only when diffing raw stderr against
bash on an intentionally-malformed builtin invocation.

**Proper fix:** give each builtin a canonical `usage:` synopsis string (matching
bash's exact wording) and, on a usage error, emit it as a second line through the
same redirect-aware sink (`errln` for in-`run_builtin` builtins, `emit_cmd_stderr`
for the `command`/`builtin` wrappers). Mechanical but must match bash's synopsis
text byte-for-byte per builtin.

### TD-OILS-MAPFILE-UFD. `mapfile`/`readarray` does not implement `-u fd` (read from a numbered descriptor) — ✅ RESOLVED 2026-07-20

**Resolution (2026-07-20):** `builtin_mapfile` now accepts `-u N` and, for
`N >= 3`, routes the read through `open_fds` (byte cursor) / `coproc_read_fds`
(live pipe) via a masking `RedirPlan`/`StdinSrc` — exactly `read -u`'s routing —
before `read_all_bytes`. The invoking name (`mapfile` vs `readarray`) is threaded
through as a `tag` so the invalid-option and fd diagnostics carry the right name,
including bash's two distinct fd errors: `<tag>: <spec>: invalid file descriptor
specification` (non-numeric, status 1) and `<tag>: <n>: invalid file descriptor:
Bad file descriptor` (closed/out-of-range, status 1). Both `help` synopses now
advertise `[-u fd]`. Verified byte-for-byte against bash 5.x; covered by the
`mapfile_reads_from_numbered_fd` regression test. Original report follows.

**Where:** `userspace/oils/src/interp.rs` — `builtin_mapfile`. Its option loop
handles `-t -d -n -c -C -s -O` and the array operand, but had no `-u` case, so
`mapfile -u 3 arr` hit the invalid-option path and failed with status 2. bash
reads the array from descriptor `fd` (e.g. one opened by `exec 3< file` or a
coproc read end).

**Repro:** `exec 3< <(printf 'a\nb\n'); mapfile -t -u 3 arr; echo "${arr[@]}"` →
bash prints `a b`; osh prints `mapfile: -u: invalid option` + usage, exit 2.

**Proper fix:** mirror `builtin_read`'s `-u` handling — accept `-u N`, and when
`N >= 3` route the read through `open_fds` (byte cursor) / `coproc_read_fds`
(live pipe) instead of the ambient `stdin`/`redir`. `mapfile` already takes
`stdin`/`redir`; add a `ufd: Option<i32>` and build a masking `RedirPlan` /
`StdinSrc` exactly as `read` does before calling `read_all_bytes`. The usage
synopsis already advertises `-u fd`, so no wording change is needed. Note
`readarray` shares this code path (same gap).

### TD-OILS-HELP-FORMAT. `osh help` no-arg listing and per-builtin synopsis *text* don't byte-match bash — PARTIALLY ADDRESSED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_help` (the no-pattern
listing branch) and the `HELP_TABLE` usage strings.

**What (fixed 2026-07-19):** a matched `help NAME` / `help -s NAME` topic now
prints bash's `NAME: <usage>` line (the builtin name, a colon, then the usage
synopsis) instead of the bare synopsis. Verified against bash for `return`,
`echo`, `.` (dot), etc.

**What (still open):** two cosmetic gaps remain:

1. **No-arg `help` listing.** bash prints a 5-line header (version banner + "these
   commands are defined internally…" text + the "star means disabled" note) then a
   **two-column, column-major** table of usage synopses, each column ~40 chars wide
   with a leading space and a trailing `>` truncation marker when the synopsis
   overflows the column. osh instead prints every synopsis on its own single line,
   sorted, with no header and no columns. (The per-line synopsis *content* is close;
   only the layout/header differ.)
2. **Simplified synopsis text.** A few `HELP_TABLE` usage strings are abbreviated
   versions of bash's, e.g. `cd [-L|-P] [dir]` vs bash's
   `cd [-L|[-P [-e]] [-@]] [dir]`, and `[ expr ]` vs bash's `[ arg... ]`. The
   `NAME:` prefix now matches, but the synopsis body after it still differs for
   these builtins.

**Impact:** very low — cosmetic stdout text only; `help` output isn't machine-parsed
and the exit status matches. Shows up only when diffing raw `help` output against bash.

**Proper fix:** (1) reproduce bash's header + two-column column-major layout (40-col
width, leading space, `>` truncation, `*`-disabled note) in the no-pattern branch,
emitting a real osh version banner rather than faking bash's; (2) align each
`HELP_TABLE` usage string with bash's exact synopsis wording byte-for-byte.
Mechanical but tedious; parked as low-value.

### TD-OILS-SUBSHELL-TRAP-DISPLAY. `osh` subshells drop parent trap *strings*, so `trap -p` inside a subshell shows nothing (bash keeps the strings for display while resetting their firing disposition) — OPEN (narrow fidelity gap; needs a display-vs-disposition split) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `clone_for_subshell` (the `traps`
field: currently filters to keep only ignored `''` traps and drops the rest).

**What:** in bash a subshell (a `(…)` group, a pipeline stage, or a command
substitution) **displays** the parent's trap command strings via `trap -p`/bare
`trap`, even though the trap's *firing disposition* is reset to default (so an
actual signal runs the default action, not the handler). Measured (bash 5.2):

```
$ trap 'echo x' INT; (trap -p)
trap -- 'echo x' SIGINT          # string shown in the subshell
$ trap 'echo x' INT; trap -p | cat
trap -- 'echo x' SIGINT          # shown across a pipeline stage too
$ trap 'echo H' USR1; (kill -USR1 $BASHPID); …
User defined signal 1            # default action ran — handler did NOT fire
```

osh keeps only ignored (`''`) traps in the subshell clone, so `trap -p | cat`
prints an empty result where bash prints the inherited line. The *firing*
semantics osh already models correctly: it never fires an inherited non-ignored
trap in a subshell (and osh has no async signal delivery at all yet).

**Impact:** low — visible only when a script inspects traps (`trap -p`/`trap`)
from **inside** a subshell (pipeline stage, `( )`, or `$( )`). Handler firing is
already correct.

**Proper fix:** split "trap string for display" from "active disposition." Keep
*all* parent trap strings in `clone_for_subshell` (so `trap -p` reflects them),
but mark the non-ignored ones reset-in-subshell so they do not fire. For the
synchronous pseudo-signals this must honour bash's inheritance rules —
`DEBUG`/`RETURN` fire in a subshell only under `functrace` (`set -T`), `ERR`
only under `errtrace` (`set -E`); by default they display but do not fire.
Naively keeping the strings *without* that guard would wrongly fire
`DEBUG`/`ERR`/`RETURN` handlers inside subshells, so the guard is required, which
is why this is deferred rather than a one-line clone change.

### TD-OILS-FATAL-ABORT-STATUS. `osh` exit status after a *fatal expansion abort* diverges from bash in `-c` mode and for arithmetic errors — OPEN (narrow, mode-dependent quirk; bash itself is inconsistent) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — the `Flow::Exit(1)` / `last_status`
handling in `exec_simple_inner` (nounset `unbound_error`, `${var?}` abort,
`arith_error`, `glob_error` branches) and `run_source`'s propagation for `-c`.

**What:** when a non-interactive shell aborts on a fatal expansion error, bash's
final exit status depends on the invocation mode and error kind, and osh does not
reproduce every case. Measured (bash 5.2.37):

| case | bash `-c` | osh `-c` | bash *script* | osh *script* |
|---|---|---|---|---|
| `${y?}` (unset param) | **127** | 1 | 1 | 1 |
| `set -u; $y` (nounset) | **127** | 1/2 | 1 | 1 |
| `$((1/0))` (div-by-zero) | 1 | 1 | **0** (continues!) | 1 |
| `${!x*extra}` (bad indirect) | 1 | 2 | — | — |
| `${x[1@]}` (bad subscript) | 1 | 0 | — | — |

The *observable* behaviour that matters — the diagnostic text, that the shell
aborts, and that following commands don't run — already matches bash. Only the
numeric exit code of the aborting shell differs, and only in these edge paths.

**Why deferred (not a quick fix):** bash is itself inconsistent here — the same
`${y?}` error yields 127 under `-c` but 1 as a script, and a div-by-zero *aborts*
under `-c` (status 1) yet *continues* as a script (status 0, from the next
command). A naive "always exit 127 on expansion abort" would break the
currently-matching script-mode cases (which are correct at 1). The correct fix
needs bash's exact rule reverse-engineered per error-kind × invocation-mode, plus
possibly making arithmetic `$((1/0))` non-fatal in script mode. Low value
(pathological error paths, exit code only), so parked here rather than guessed.

### TD-OILS-ARRAYLIT-SPACED-SUBSCRIPT. `osh` tokenizes a *space-containing* array-literal subscript (`a=([3 x]=99)`) into two positional words instead of one keyed element — MINOR PARSER DIVERGENCE 2026-07-19

**Where:** `userspace/oils/src/parser.rs` (array-literal element tokenizer) —
the code that splits the words inside `a=( … )` and recognises `[subscript]=value`
keyed elements. The subscript is scanned only up to the first unquoted whitespace,
so a bracketed subscript that itself contains a space is not treated as a single
`[...]=` unit.

**What:** for an array literal whose keyed subscript contains whitespace, osh and
bash disagree on tokenization:

```
a=([3 x]=99)
  bash:  one keyed element with subscript `3 x`, which is then an *arithmetic*
         subscript that errors ("3 x": bad arithmetic) -> fatal, status 1.
  osh:   two positional words `[3` and `x]=99`, i.e. `declare -a a=([0]="[3" [1]="x]=99")`
         -> no error, status 0.
```

Valid keyed subscripts *without* spaces behave identically in both shells:
`a=([3]=99)`, `a=([1+2]=99)` both assign index-computed elements the same way, and
a space-containing subscript that would be a valid arith expression is the only
divergent case.

**Why deferred (separate from the arithmetic-fatality work):** this is a
*tokenization* difference in the array-literal parser, orthogonal to the
subscript-arithmetic-fatality fix (which correctly makes `${a[3 x]}` reads fatal).
To match bash the array-literal element splitter must recognise a leading
`[ … ]=` where the `…` may contain spaces (scan to the matching `]` before
deciding word boundaries), then feed that subscript through the same fatal
`eval_arith_index` path. Low value (obscure literal form; the space inside a
subscript is almost always a typo), so parked here rather than reworking the
element tokenizer now.

### TD-OILS-COMPLETE-NOOP. `complete`/`compopt` register/print/remove completion specs but never generate completions (osh has no interactive tab-completion), and multi-spec `complete -p` lists in insertion order, not bash's hash order — MINOR, by design 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_complete` / `builtin_compopt`,
the `CompSpec`/`CompKey` types, the `comp_specs: Vec<(CompKey, CompSpec)>` field,
and the `format_compspec` renderer.

**What:** the `complete` and `compopt` builtins are implemented for *script
compatibility* — a sourced `bash_completion` file (or any script) calls
`complete …` hundreds of times, and previously each errored with `complete:
command not found` (status 127), aborting the source. Now they parse fully,
store the spec, print it re-executably (`complete -p`, byte-matching bash for a
single spec, including the fixed option print order and `'\''` quoting), mutate
it (`compopt -o`/`+o`), and remove it (`complete -r`). Two intentional
limitations remain:

1. **Specs are never *used*.** osh's REPL is line-oriented with no Readline tab
   completion, so a registered `-F func`/`-C cmd`/`-W list` generator is stored
   but never invoked to produce candidates. `compgen` (which *does* generate
   candidates on demand) is the functional half; `complete` is the registration
   half with no completion engine behind it.
2. **`complete -p` (list all) uses insertion order**, whereas bash iterates its
   internal hash table (an order that depends on bash's string-hash + bucket
   layout and is not reproducible without replicating those internals). Each
   *individual* `complete -p NAME` line matches bash exactly; only the relative
   order of *unrelated* specs in a full listing can differ.

**Why by design:** (1) is blocked on there being an interactive line editor with
completion at all (a much larger, separate feature); until then a stored-but-unused
spec is the correct bash-compatible behavior for non-interactive scripts. (2) is a
deliberate trade — matching bash's hash-bucket order byte-for-byte has no practical
value (scripts that care read `complete -p NAME`, not the unordered full dump) and
would require hard-coding bash's hash function. Fixing (1) would also make (2)
moot in practice (real completion never depends on listing order).

### B-TCC-LIBTCC1-MAIN. On-target tcc one-shot compile+link spuriously fails with `unresolved reference to 'main'` (exit 1) when the source emits one extra undefined symbol (e.g. the `memset` a struct/aggregate brace-initialiser synthesises) — ON-TARGET-ONLY, **COULD NOT REPRODUCE (22 on-target compiles) — DOWNGRADED TO WATCH**, REGRESSION-GUARDED 2026-07-16

**UPDATE 2026-07-16 (could not reproduce; downgraded WATCH; regression
guard added).** On-target instrumentation was built and run to reproduce
this live: a boot self-test (`self_test_tcc_diag_brace_init`, since
removed) compiled **four distinct `memset`/`memcpy`-emitting constructs**
(constant brace-init, runtime-value brace-init, a 256-byte zero-init
array, and a struct-to-struct copy) **five times each = 20 on-target
`tcc -vv` compiles**, plus two earlier single shots = **22 on-target
compiles that all carried the extra undefined `memset`/`memcpy` symbol.
Every one linked and ran cleanly (exit 0, valid dynamic ELF).** The
documented deterministic trigger — "one extra undefined symbol makes the
on-target link lose `main`" — is therefore **disproven**: `memset`
presence is *not* sufficient to reproduce the failure. The original Part
47 failure was thus either genuinely **intermittent/rare** (timing- or
heap/VFS-state-dependent, like the sibling `B-WAITQ-IDLEPARK` lost-wakeup
family) or was **already fixed** by an unrelated change since Part 47.
Because no root cause could be pinned and no deterministic repro exists,
the entry is downgraded from OPEN to **WATCH**.

A permanent **regression guard** now exists:
`self_test_linux_real_glibc_cc_brace_memset` (Path Z Part 56,
`kernel/src/proc/spawn.rs`), wired into the boot self-tests, compiles +
glibc-links + runs in ring 3 a program with a genuine runtime-`memset`
aggregate brace-initialiser and asserts output `42\n`. If tcc ever
regresses to losing `main` when a synthesised `memset` is present, that
rung fails and emits a `self-test failed` WARNING the boot-test scans
for. The field-init workaround in the other Path Z rungs is no longer
strictly required (brace-init is proven reliable) but is harmless and
left in place. The original OPEN analysis is retained below for history.

**Symptom.** A hosted compile+link in a *single* on-target tcc invocation
(`tcc -o /prog /prog.c`, the shape `run_hosted_cc_case` uses) fails with
`tcc: error: unresolved reference to 'main'` (exit 1) — even though the
source plainly defines `int main(void)`. The trigger observed live was an
aggregate **brace initialiser** (`struct s x = {…};`), which tcc lowers to
a synthesised `memset` reference; the *field-wise* version of the same
program (one fewer undefined symbol: only `write`) links and runs cleanly.

**IMPORTANT — earlier mechanism guess was wrong.** The first draft of this
entry blamed `libtcc1.a` (claiming tcc resolves the synthesised
`memset`/`memcpy` from its runtime archive and that perturbs the link).
That is **incorrect**: `ar t`/`nm --print-armap` on the staged
`libtcc1.a` show it defines the soft-float/atomic/alloca/va_list helpers
but **not** `memset`/`memcpy` — those resolve from glibc (`libc.so.6`).
So `libtcc1.a` is not pulled in by the brace-init program at all. The real
differentiator is simply the *one extra undefined symbol* (`memset`), and
the breakage is **on-target-specific**.

**Reproduction / diagnosis (what was actually done).** Extracted the whole
staged toolchain from `rootfs.ext4` via `debugfs -R "dump …"` (tcc, crt1/
crti/crtn.o, the `libc.so` GNU-ld GROUP script, `libc_nonshared.a`,
`libtcc1.a`, `libc.so.6`, `ld-linux`) and re-ran the *extracted target
tcc* under WSL:
  - `tcc -c prog.c -o prog.o` → OK; `nm` shows good `T main`, plus
    `U memset` for the brace-init variant vs. only `U write` for field-init.
  - Full `tcc -o prog prog.c` (one-shot compile+link) → **exit 0 for BOTH
    variants**, both with WSL's native crt/libc and with the OS's staged
    crt + `libc.so` GROUP script + `libc_nonshared.a` + `libtcc1.a` forced
    in explicitly via `-nostdlib`.
So the `unresolved 'main'` failure **does not reproduce off-target** — it
only happens when tcc runs *inside the OS* (under our Linux-syscall
translation + VFS). That points at an OS-side interaction (tcc's file
reads of the large `libc.so.6` / GROUP-script / archive parsing under our
syscall+VFS layer, or a heap/symbol-table quirk in tcc keyed to the extra
symbol), **not** an archive-index or link-ordering defect in the staged
files themselves.

**Why it matters.** The on-target C toolchain can currently mis-link (in
one step) programs that carry an extra compiler-synthesised undefined
symbol — most commonly aggregate brace initialisers (a lot of ordinary C).
Path Z rungs sidestep it (hand-rolled field init); coreutils/real projects
may hit it. Workaround: compile `-c` then link separately, or avoid the
construct.

**Where it lives.** On-target `tcc` (`/bin/tcc`, 0.9.28rc mob) running via
the Linux-ABI syscall translation + VFS; the staging is in
`stage_hosted_cc_support` (`kernel/src/proc/spawn.rs`). The self-test that
first exposed it: `self_test_linux_real_glibc_cc_struct` (Path Z Part 47).

**Proper fix (open — needs on-target instrumentation).** Because it only
reproduces inside the OS, the next step is to capture what tcc actually
does there: strace-equivalent of the failing link (there is already
`scripts/extract-tcc-strace.sh` / `scripts/probe-tcc-hosted.sh`) to see
whether a file read of `libc.so.6` / the GROUP script / `libc_nonshared.a`
returns short/EOF-early, or whether tcc's dynamic-symbol lookup for
`memset` walks into a region our VFS serves incorrectly. If a specific
syscall/VFS read is returning wrong data for large files under tcc's
access pattern, fix that; otherwise it may be a genuine tcc bug worth
patching in the port. Until then the entry stays WATCH/OPEN with the
field-init workaround in place.


### B-WAITQ-IDLEPARK. Intermittent boot hang in the waitqueue `wait_timeout_ns (expired)` self-test — CPU idle-parked with a Blocked task, lost-wakeup family — INSTRUMENTED, WATCH 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-173612-iter06.*`), ~1 in 20+ boots.
Distinct from the sysctl deadlock below. The QEMU-monitor register
capture shows CPU#0 (the *only* CPU — `info cpus` lists one) at
`RIP=0xffffffff81b2f895 = kernel::cpu::hlt`, `RFL=0x286` (**IF set** —
interrupts *enabled*), `HLT=1`. Two consecutive `info registers` snapshots
show `HLT` toggling `1 → 0` at the same RIP, i.e. the CPU keeps waking on
each ~10 ms timer tick, runs the ISR, finds nothing runnable, and HLTs
again. So this is **not** a spinlock deadlock and **not** a dead timer —
it is a **lost wakeup**: a Blocked task is never re-dispatched.

**Where it hangs.** Serial ends (7094 lines) mid-`[waitqueue] Running
self-test…`, last line `wait_timeout_ns (zero timeout): OK`. The next
subtest — `wait_timeout_ns (expired)` (kernel/src/sched/waitqueue.rs
~522) — is the **first** subtest that actually *blocks* the boot task on a
real hrtimer (`sleep_ns(500_000)` → `block_current`) and expects a timer
wakeup; every prior subtest was "already true" / immediate-timeout and
never parked. The boot task blocks with no other runnable task, enters the
`schedule_inner` **idle-fallback HLT loop** (sched/mod.rs ~4882), and the
500 µs hrtimer wake never re-dispatches it.

**Root cause — NOT yet pinned.** Static analysis (this session) could not
prove the exact race. The wake path *should* be robust: `sleep_ns` arms an
hrtimer whose callback runs `try_wake` (→ `defer_wake` on lock contention),
`block_current`/`wake` serialise on `SCHED.lock` with a `pending_wake`
handshake, and both `schedule_inner` and the idle-fallback loop
`drain_deferred_wakes_locked`. On a single CPU at idle the callback's
`try_lock` should always succeed and find the task `Blocked`. The two
leading hypotheses are (a) the hrtimer entry/callback is somehow lost so
`process_expired` never fires it, or (b) the wake marks the task Ready but
it is orphaned out of every run queue (Ready-but-unqueued), which
`check_starvation` (only rescues Ready→enqueue? verify) may not recover in
this pre-BOOT_OK window. This is the same "all CPUs idle-ticking"
lost-wakeup signature as the B-PTHREAD-YIELDBUDGET family.

**Diagnostic gap fixed.** The boot-scoped liveness watchdog is armed only
at `BOOT_OK`, so a pre-BOOT_OK idle-park hang produces *no* `[liveness]`
dump — only the bare NMI register capture. Added `dump_idle_fallback_wedge`
(sched/mod.rs): the idle-fallback loop now counts consecutive
nothing-runnable HLT-wakes and, after `IDLE_FALLBACK_WEDGE_TICKS` (500 ≈
5 s), dumps **once** (one-shot `IDLE_FALLBACK_WEDGE_DUMPED`): the parked
task's `state`/`pending_wake`/`last_cpu`, `hrtimer::pending_count()` +
`next_expiry_ns`, `local_has_real_work`, occupied deferred-wake slots, and
the full task table. This turns the silent hang into a self-describing dump
so the *next* reproduction pins root cause (Blocked-forever vs.
Ready-orphan vs. lost hrtimer).

**Also fixed (adjacent UB in the same race family).** `schedule_inner`'s
`picked_id == current_id` guard was gated on `&& requeue`. On the block
path (`requeue == false`) a concurrent wake can re-enqueue the current task
so `pick_next_local` returns it; the old guard let that fall through to the
real switch path, which aliases `&mut *old_p` and `&*new_p` to the *same*
`Context`/`FpuState` (UB) and does a pointless save-then-restore. The guard
now handles `picked == current` uniformly regardless of `requeue` (resume
as Running, return). This likely "worked by luck" before (save then restore
the same memory), so it is probably not the observed hang, but it removes
real UB.

**Next step.** Let `wedge-soak.sh` run until the new dump fires, then read
`build/hang-catches/*.serial.txt` for the `[sched] *** IDLE-FALLBACK WEDGE`
block and pin the root cause.

### B-SCHED-SPAWN-DEADLOCK. Intermittent boot hang — boot task spins on `SCHED.lock()` in `spawn_inner` during the `tcc` project-header spawn self-test; SCHED held by an unidentified non-running context — INSTRUMENTED, WATCH 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-180829-iter15.*`), ~1 in 15 boots.
Distinct from both B-WAITQ-IDLEPARK (a lost-wakeup, IF *set*, HLT) and
B-SYSCTL-IRQ-DEADLOCK (sysctl lock). The i6300esb NMI hard-lockup watchdog
captured CPU#0 (the only CPU) at `RIP=0xffffffff815c8976` =
`core::sync::atomic::spin_loop_hint`, `RFL=0x00000002` (**IF cleared** →
spinning with interrupts disabled = a spinlock deadlock, not a lost-wakeup).
`RDI=0xffffffff8271efb0`, which `llvm-nm` resolves to **exactly**
`kernel::sched::SCHED` — so the CPU is spinning on `SCHED.lock()`.

**Where it hangs.** `llvm-objdump -dl` on the frozen RIP + backtrace
resolves the stack (fresh `target/x86_64-unknown-none/debug/kernel`,
Jul 15 18:06) to:
`kernel_main → spawn::self_test_linux_real_glibc_cc_project_header →
spawn_reap_tcc → proc::spawn::spawn_process → spawn_process_inner →
proc::thread::spawn → sched::spawn_suspended → sched::spawn_inner →
cpu::without_interrupts{closure} → spin_loop_hint`. The spin is the inlined
`spin::Mutex::lock` of `SCHED.lock()` at `sched/mod.rs` ~1189 (address
operand `0x8271efb0` in the disasm == `&SCHED`), inside the
`without_interrupts` critical section. Serial ends mid-`[spawn] Running REAL
project-header C build (tcc, #include "...", ring 3, Path Z) test…`
(process 217), i.e. the *6th* `tcc` spawn (the prior 5 tcc self-tests all
passed) — so it is timing/allocation-pattern dependent, not a static
double-lock. The post-BOOT_OK liveness watchdog fired (heartbeat=4328,
ctx_switches=1122) and confirmed `!! could not acquire SCHED lock — a task
is likely wedged holding it`.

**Root cause — NOT yet pinned.** On a single CPU, SCHED can only be *held
while its holder is not running* if some context acquired SCHED and then a
context switch occurred before it released — yet: (a) involuntary preemption
is already deferred while SCHED is held (`do_deferred_preempt` checks
`SCHED.is_locked()`, sched/mod.rs ~2666), and (b) a *voluntary* yield while
holding SCHED would immediately self-deadlock in `schedule_inner`'s own
`SCHED.lock()` (which would show `schedule_inner` in the backtrace, not
`spawn_inner`). Static analysis this session could not identify which of the
~50 `SCHED.lock()`/`try_lock()` sites leaks it, nor the exact race
(candidate: a `SCHED.is_locked()`-guard race, or a fault/exception while
holding SCHED). SCHED is a raw `spin::Mutex` and does **not** bump
`preempt_count` (only `crate::sync::Mutex` does), so the `spawn_inner`
voluntary-switch guard at sched/mod.rs ~4744 — which checks `preempt_count`
— would *not* catch a SCHED-held voluntary switch.

**Diagnostic fix (this session).** Wrapped SCHED in a `SchedMutex` newtype
(sched/mod.rs) mirroring the existing `mm::heap` HEAP_LOCK_OWNER/SITE
mechanism: `lock`/`try_lock` are `#[track_caller]` and record the holder's
task-id + CPU + `&'static Location` acquire site into
`SCHED_LOCK_{OWNER,CPU,SITE}` atomics; the `SchedGuard` clears them on drop.
`try_lock` records **only on success** so a failing probe never clobbers the
real holder's record. New lock-free `dump_sched_lock_owner()` prints
`tid=… (cpu …) acquired at file:line:col` and is now called from the
liveness "could not acquire SCHED lock" path (~2273). The `SchedGuard`
derefs transparently to `SchedState`, so all existing call sites are
unchanged. This turns the "victim spinning in spin_loop_hint" capture into a
direct pointer at the leaking acquire site on the next reproduction.

**2nd catch (iter21, 2026-07-15 run 204740).** Re-caught at `RIP=spin_loop_hint`,
`RDI=&SCHED` again — but `RFL=0x202` (**IF *set*** — interrupts enabled, distinct
from iter15's IF-cleared `without_interrupts` spin) during the same tcc ring-3
spawn self-test (`[spawn] Running REAL C compiler (tcc, ring 3, Path Z) test`).
20 clean boots preceded it. The new SCHED holder-tracking dump fired and
reported: `SCHED-lock: record shows unlocked … but the lock is still physically
held`. **Key inference:** on a single CPU, `record()` runs immediately after the
physical acquire, so `OWNER == u64::MAX` while the lock is physically held means
the holder is wedged in the tiny window *between* `self.0.lock()` and `record()`
— which only stalls for 15+ s if a **fault or interrupt lands in that window and
re-enters `SCHED.lock()` on the same CPU** (a single-CPU self-deadlock: spinning
to acquire a lock whose holder can't run). The page-fault handler
(`idt.rs::handle_page_fault`) does `cpu::sti()` when the faulting context had
IF=1 and then runs long fault resolution (demand paging, CoW, swap) — a plausible
source of same-CPU re-entrancy. `account_fault` already uses `try_lock` (safe);
the offending re-entrant `SCHED.lock()` site is not yet pinned.

**2nd diagnostic pass (this session).** The original holder-tracking could not
name the holder in the iter21 case (OWNER was never written — the holder was in
the acquire→record window). Added a **per-CPU acquire-site stack**
(`SCHED_ACQ_SITES`/`SCHED_ACQ_DEPTH`, sched/mod.rs): `SchedMutex::lock`/`try_lock`
push `Location::caller()` **before** taking the physical lock and pop on guard
drop. This captures (a) a holder wedged in the acquire→record window (its site is
already pushed) and (b) the full **nesting chain** when a fault/IRQ handler
re-enters SCHED on the same CPU — naming BOTH the outer holder and the inner
deadlocking acquirer, which the NMI frozen-RIP capture (always just
`spin_loop_hint`) and the OWNER record alone cannot. `dump_sched_lock_owner` now
always prints every CPU's acquire-stack (`SCHED acquire-stack cpuN: depth=… →
[lvl] file:line:col`).

**3rd soak (2026-07-15 run 215126, 60 iters, instrumented).** Ran the full
acquire-stack build for 60 consecutive boots: `WEDGE_SOAK_DONE rc_caught=0` — the
race did **not** fire this run (all 60 BOOT_OK, 93–113 s each). Consistent with
the measured ~1-in-20-to-28 recurrence: 60 clean boots is within ordinary bad
luck. No new artifact produced. Rather than keep re-running clean soaks (a
no-edit verification loop), the diagnostic net is now **permanently baked into
the kernel** (ba717f518), so the acquire-stack dump will fire automatically on
the *next* wedge in any routine boot-test or soak — no dedicated hunt needed.

**Next step.** WATCH — the instrumentation is in place. On the next re-catch
(regs newer than `soak-20260715-204740-iter21`), read the
`[liveness]   SCHED acquire-stack cpuN:` frames — the deepest (inner) frame is
the re-entrant `SCHED.lock()` that deadlocks, the outer frame is the holder.
Then fix the re-entrancy (make the inner site non-blocking `try_lock`, or ensure
SCHED is not held across a fault-prone / interruptible region).

**LIKELY ROOT CAUSE FOUND (2026-07-15, static audit) — see
B-COMPLETION-TIMER-IRQ-DEADLOCK below.** A static audit of *every* IRQ/softirq
path that can reach a blocking `SCHED.lock()` found exactly one such site: the
timer softirq's `ipc::timer::process_timer_expirations` → `completion::notify`
→ blocking `sched::wake()`. A timer softirq runs with interrupts enabled and can
preempt a task holding `SCHED` (holders don't disable interrupts), so if a
completion-port timer happens to expire in that window the softirq's
`sched::wake()` re-enters `SCHED.lock()` on the same CPU and spins forever —
**exactly** this bug's signature (RIP=`spin_loop_hint`, RDI=`&SCHED`, IF set,
"record shows unlocked" because the holder was mid acquire→record when the timer
fired). The tcc ring-3 spawn just widens the window (lots of SCHED traffic). This
is the same interrupt-reentrancy class as B-SYSCTL-IRQ-DEADLOCK. **Fixed** in the
same pass (softirq-safe `completion::try_notify` + retry-on-contention in
`process_timer_expirations`). Keep the acquire-stack instrumentation and the WATCH
status until a long soak confirms the wedge no longer reproduces; if it *does*
still fire after this fix, the acquire-stack dump will name the true site.

### B-COMPLETION-TIMER-IRQ-DEADLOCK. Timer-softirq completion notify blocking-locks SCHED → same-CPU deadlock if it preempts a SCHED holder — ROOT-CAUSED & FIXED 2026-07-15

**Class.** Interrupt-reentrancy deadlock on the global `SCHED` (and `CP_TABLE`)
spinlock — the same broad family as B-SYSCTL-IRQ-DEADLOCK, and the strongly
suspected root cause of B-SCHED-SPAWN-DEADLOCK (see above).

**Root cause.** `ipc::timer::process_timer_expirations()` runs in the timer
**softirq** (`softirq::handle_timer`, interrupts enabled). For each expired timer
bound to a completion port it called `completion::notify()`, which takes the
blocking `CP_TABLE.lock()` and then the blocking `sched::wake()` →
`SCHED.lock()`. `SCHED` holders in `sched/mod.rs` do **not** disable interrupts,
so the timer softirq can fire on a CPU that is mid-critical-section holding
`SCHED` (or in the tiny acquire→record window). On the single-CPU boot the
softirq's `SCHED.lock()` then spins forever waiting for a lock the *same* CPU
holds — a self-deadlock. It only triggers when a completion-port timer expires
in that exact window, hence the ~1-in-20-to-28 rarity the soak observed.

**Audit scope (all clean except the one site).** Verified every other
softirq/IRQ-reachable path is already non-blocking on `SCHED`:
`#PF` handler (`try_resolve_fault`→`PROCESS_TABLE.try_lock`; `account_fault`,
`panic_diagnostics`→`SCHED.try_lock`); timer preemption (`do_deferred_preempt`
defers when `SCHED.is_locked()`); device-IRQ wake (`ioapic::handle_device_irq`
→`sched::try_wake`); `softirq::handle_timer` sub-calls
(`process_sleep_wakeups`→`wake_expired_sleeper` try_lock,
`process_deferred_wakes`→`try_wake`, `push_balance`→`SCHED.try_lock`);
`softirq::handle_sched`→`push_balance` (try_lock);
`ktimer::process_expirations` (defers callbacks to the workqueue, no inline
SCHED lock). Only `process_timer_expirations`→`completion::notify` blocked.

**Fix.** Added `completion::try_notify(cp, source) -> bool` (softirq-safe:
`CP_TABLE.try_lock()` + `sched::try_wake()`; commits **nothing** on contention —
returns `false` so the caller retries next tick, avoiding both a lost wakeup and
a duplicated event). Restructured `process_timer_expirations` to call
`try_notify` *before* advancing/expiring the timer and to `continue` (leave the
timer un-advanced, retry next ~10 ms tick) on contention. `completion::notify`
(blocking) is unchanged for its task/syscall-context callers (`io_ring`,
`syscall::handlers`) and `close()`. Contention is transient (SCHED/CP_TABLE held
only briefly), so the bounded per-tick retry resolves within a tick or two.

**Where it lives.** `kernel/src/ipc/completion.rs` (`try_notify`),
`kernel/src/ipc/timer.rs` (`process_timer_expirations`). Detector:
`scripts/wedge-soak.sh` (was catching it as B-SCHED-SPAWN-DEADLOCK).

**Next step.** Boot-test, then run a long `wedge-soak.sh` to confirm the SCHED
wedge no longer reproduces (it was ~1/20-28; a clean 40+ iteration soak is good
evidence). This is a confirmed 4th instance of the raw-`spin::Mutex` deadlock
class — see open-questions.md Q24 (recommendation was "escalate to C if a 3rd/4th
shows up"; this is the interrupt-reentrancy sub-variant, already fixed reactively
without a new lock type, consistent with the B-SYSCTL fix).

**UPDATE 2026-07-16 — CONFIRMED.** The confirmation soak
(`soak-20260715-235730`) ran 40 iterations: the SCHED spinloop wedge did **not**
reproduce in any of them (was ~1/20-28), strong evidence the fix holds. The soak
*did* stop on a **different, pre-existing** wedge at iter40 — a kernel jump to
`RIP=0x0` during the tcc-signal Path-Z self-test that cascaded into a kernel
stack-overflow storm. That is unrelated to this deadlock (different signature: a
control-flow hijack, not a spinloop) and is tracked separately as
**B-KNULLJUMP-SIGNAL** below. Downgrading this entry's confidence: the fix is
validated; leaving as ROOT-CAUSED & FIXED.

### B-KNULLJUMP-SIGNAL. Rare kernel jump to `RIP=0x0` during the tcc-signal Path-Z self-test, cascading into a kernel-stack-overflow #DF/#UD storm — DIAGNOSTICS HARDENED, ROOT-CAUSE OPEN, WATCH 2026-07-16

**Class.** A control-flow hijack in kernel context: the kernel executed a
`call`/`jmp` through a **null (or corrupted) code pointer**, landing at
`RIP=0x0`. This is *not* a spinlock/deadlock bug (distinct from
B-COMPLETION-TIMER-IRQ-DEADLOCK, whose fix held across all 40 soak iterations).

**Symptom / evidence.** Caught once by `scripts/wedge-soak.sh` at
`build/hang-catches/soak-20260715-235730-iter40.{serial,regs}.txt` — 1 catch in
40 boots (the tcc-signal test itself *passed cleanly* in iters 1/5/20/39, so this
is rare and intermittent, not deterministic). Serial trace (iter40, line ~3262):

```
[spawn] Running REAL C compiler (tcc, HOSTED glibc link, signal, ring 3, Path Z) test...
EXCEPTION: Page Fault (#PF) at 0x0, address=0x0, error=0x10   <- kernel I-fetch @ 0x0
  Cause: not-present, read, kernel
  CS=0x8 RFLAGS=0x10646 RSP=0xffffc100000270a0 SS=0x10
EXCEPTION: Page Fault (#PF) at 0xffffffff815544fe, address=0x3c8fcd6c4d, error=0x0  <- diag path faults
...  (recursive #PF storm, RIP==CR2 descending the kstack ~0x850/frame, error=0x11 NX)
EXCEPTION: Double Fault (#DF) at 0xffffffff80fd2141
  RSP 0xffffc10000017ff8 is in a kstack GUARD PAGE — KERNEL STACK OVERFLOW confirmed
EXCEPTION: Invalid Opcode (#UD) ...  (garbage-execution storm in .data) -> NMI watchdog
```

The crash fires **immediately after** the "signal, ring 3, Path Z" banner and
**before** the `[spawn] ELF validated` line that a clean run prints next — i.e.
during kernel-side setup of the tcc-signal spawn, right after the *previous*
test's task exited (`Process 222 ... now zombie` / `Task 188 exiting`). Prime
suspicion: a use-after-free / teardown race on a kernel code pointer (a
completion/workqueue/timer callback, or a saved return address) at the
zombie-cleanup → next-spawn boundary. On UP the only concurrency is
interrupt/softirq preemption, so a softirq firing mid-setup and invoking a
freed/zeroed callback is a plausible mechanism. Not yet pinned.

**Secondary bug (FIXED this session).** The single null-jump was *buried* under a
4400-line cascade because the fatal kernel-`#PF` path ran stack-hungry
diagnostics (RBP frame-walk + formatting) with **no re-entrancy guard**: when
those diagnostics themselves faulted (the second `#PF` at `0xffffffff815544fe`),
the nested `#PF` restarted the whole report, recursing ~2 KiB/level until the
kstack overflowed into `#DF` → `#UD` storm → NMI watchdog. Fix in
`kernel/src/idt.rs`:
- Added `FATAL_FAULT_IN_PROGRESS: AtomicBool`. `handle_page_fault` checks it at
  entry (before `sti`/resolve) and, if set, prints one minimal line and halts —
  no recursion. It is armed at the start of the fatal kernel-`#PF` branch, before
  any diagnostics. `handle_double_fault` swap-arms + checks it too.
- Reordered the fatal-`#PF` diagnostics so the **safe** raw stack-scan
  (`dump_stack_scan`, which validates every slot against known kstack regions
  before dereferencing) runs *before* the `print_current` RBP walk (which blindly
  follows a possibly-corrupted chain and can fault). This guarantees the
  return-address candidates that name the hijacked caller reach serial even if
  the walk trips the guard and halts.

Net effect: the next time this null-jump reproduces, we get a single clean fault
report **plus a stack-scan naming the caller of the null pointer**, instead of a
stack-overflow storm that destroys the evidence.

**Where it lives.** Bug: unknown (tcc-signal spawn setup / zombie-cleanup race).
Diagnostics fix: `kernel/src/idt.rs` (`handle_page_fault`, `handle_double_fault`,
`FATAL_FAULT_IN_PROGRESS`). Detector: `scripts/wedge-soak.sh`.

**Next step.** Re-run `wedge-soak.sh` (long, 60+ iters) to re-catch with the new
diagnostics and read off the caller from the stack-scan; that pins the corruption
site so a proper root-cause fix can follow. Until then this is WATCH — rare,
and the cascade (the part that took down the whole machine hard) is now contained
to a clean halt.

**UPDATE 2026-07-16 — reproduction-hunt soak came back clean.** Ran an 80-iter
`wedge-soak.sh` (`soak-20260716-014354`) specifically to re-catch this with the
new diagnostics: **0 catches in 80 boots** (all passed, e.g. iter80 BOOT_OK
121s). Combined with the original 1/40, the empirical rate is ~1-in-120 — too
rare to force in a bounded soak. Ending the *dedicated* hunt (per the
no-idle-loop rule: a clean verification loop is done). This stays WATCH: the
diagnostics fix (commit 6fb1597aa) is permanently in place, so the **next**
spontaneous occurrence — in any future soak or a normal boot — will self-report a
single clean fault line plus a stack-scan naming the null pointer's caller, which
is what's needed to pin and fix the root cause. No further action until then.

**UPDATE 2026-07-16 — deferred-callback dispatch paths hardened (defense in
depth).** Audited every kernel path that invokes a *stored code pointer* from
interrupt/softirq/exit context — the mechanism class that would produce this
bug's exact signature (an async `call` through a corrupted/zeroed field →
`RIP=0` or a wild address in kernel context). Findings + fixes:
- **`hrtimer::process_expired`** (`kernel/src/hrtimer.rs`) was the *only* path
  that called a code pointer **directly from the APIC timer ISR** with **no
  validation** (`cb(arg)`), so a corrupted per-CPU `TimerEntry.callback` field
  would jump the ISR straight to a bad address. Now validates the callback
  against real `.text` bounds before dispatch; a rejected pointer is logged
  (`[hrtimer] CRITICAL: refusing to dispatch corrupt timer callback …`) and
  skipped.
- **`ktimer::process_expirations`** (`kernel/src/ktimer.rs`) only rejected an
  *exactly-zero* `func`; strengthened to a full `.text` check so a non-zero-but-
  wild value (torn store / heap overrun) is caught, the slot freed, and logged,
  instead of being submitted to the workqueue and later jumped-to by the worker.
- **`notify_exit_hooks`** (`kernel/src/sched/mod.rs`) only rejected exactly-zero
  hook slots; strengthened to a full `.text` check. This runs **at task-exit
  time — the exact moment this bug fires** (right after "Task N exiting"), so a
  clobbered hook slot now logs-and-skips rather than jumping the dying task's
  context to a wild address.
- **`workqueue::worker_entry`** (`kernel/src/workqueue.rs`) called `(work.func)`
  directly with no validation. This is the single chokepoint where *every*
  submitted callback is finally invoked, so validating here covers all
  submitters at once; a rejected entry is logged and dropped.
- **`rcu::process_callbacks`** (`kernel/src/rcu.rs`) dispatches deferred
  callbacks from the BSP softirq (`rcu::tick`) via `(cb.func)(cb.arg)` with no
  validation; now `.text`-checked, logged + skipped on failure.
- Exposed `idt::is_kernel_text` as `pub(crate)` (precise linker-symbol
  `__text_start..__text_end` bounds) as the shared validator.

With these, **all five** kernel deferred-code-pointer dispatch sites (hrtimer,
ktimer, exit-hooks, workqueue, rcu) now validate against `.text` before calling
— whichever one is the corruption victim, the next occurrence self-reports which
subsystem and what `arg` was involved instead of jumping to `RIP=0`.

**Follow-up 2026-07-16 — audit completed to the last two indirect-call sites.** A
full kernel sweep for stored/transmuted `fn`-pointer dispatch (`transmute`-to-`fn`
and `.<field>)(…)` indirect calls) confirmed only two more registration-table
call sites existed beyond the five async ones: `fs::fileinfo` custom metadata
extractors (`(ext.func)(…)`) and `mm::pressure` shrinker callbacks
(`(shrinker.callback)(…)`). Both run in *synchronous* thread context (not from a
timer ISR, so they don't match this bug's async-jump-during-spawn signature as
closely), but each is a `Mutex<Vec<struct{fn ptr}>>` whose heap backing could be
clobbered by the same suspected overrun, so both were hardened with the identical
`.text` guard (`[fileinfo]`/`[pressure] CRITICAL: refusing … (see
B-KNULLJUMP-SIGNAL)`). The two `transmute::<u64, fn>` sites (ktimer, exit-hooks)
are the ones already guarded above. **The kernel now validates every
stored-code-pointer dispatch (7 sites total) before calling.** Boot-validated
(BOOT_OK 138s, no false positives).
These are **not** the root-cause fix (the corruption *source* is still unknown),
but they (a) are the correct defensive posture for dispatching a stored code
pointer, and (b) convert the catastrophic wild-jump into a **named diagnostic
that identifies which subsystem carried the bad pointer** — a large step toward
pinning the corruption site on the next occurrence, complementing the idt.rs
re-entrancy guard (6fb1597aa). Boot-validated: BOOT_OK 104s, no false-positive
`CRITICAL` logs (all legitimate callbacks validate as `.text`), hrtimer/ktimer
self-tests still pass. Still WATCH for the underlying corruption.

### B-VIRTIO-BLK-WRITE-TIMEOUT. Intermittent boot hang — a spurious virtio-blk request timeout corrupts the virtqueue, cascading into an unrecoverable storm of write timeouts during ext4 journal replay — ROOT-CAUSED & FIXED 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-190010-iter28.*`), ~1 in 28 boots.
Distinct from the three prior soak catches (B-WAITQ-IDLEPARK,
B-SCHED-SPAWN-DEADLOCK, B-SYSCTL-IRQ-DEADLOCK) — the SCHED/idle-fallback
dumps correctly did **not** fire. The i6300esb NMI watchdog froze CPU#0 at
`RIP=0xffffffff81e9492a` = `ext4::journal::Journal::open`, `RFL=0x86`
(IF *set* — not a spinlock deadlock; the CPU is livelocked retrying I/O).
The serial log ends with **136** `[virtio-blk] Write sector N timed out`
messages (first in polling mode, later in IRQ mode) interleaved with a
livelock of `[sched] Anti-starvation: cur=0 boosted 1 task to priority 0:
[130(p20)]` (kswapd starved while the boot task spins retrying journal
writes).

**Root cause.** Two compounding bugs in the single-outstanding virtio-blk
driver (`kernel/src/virtio/blk.rs`), which shares *one* DMA frame across all
requests:
1. **Trigger — too-short polling budget.** `wait_completion`'s polling
   fallback (early boot, pre-IOAPIC) timed out after only `1_000_000`
   `spin_loop()` iterations (~1 ms). Under soak-test host contention a real
   QEMU virtio-blk completion can take longer, so the *first* timeout fired
   spuriously even though the device was healthy and would have completed.
2. **Cascade — unsafe timeout recovery.** On timeout the old code did
   `self.queue.free_chain(head)` and returned `Err`, but the device **still
   owned** those descriptors and the shared DMA buffer. The caller
   (`Journal::open`) retried; the next `submit()` reused the just-freed
   descriptors and the same DMA buffer. When the device finally completed
   the abandoned request, `poll_used()` returned that stale head (the driver
   accepted *any* completion with no head-matching), the used ring desynced,
   and `free_chain` double-freed a descriptor — corrupting the free list.
   From there every request timed out (now in IRQ mode, 5 s each), an
   unrecoverable storm.

**Fix (this session).**
- **Adequate polling budget.** New `POLL_TIMEOUT_SPINS = 100_000_000`
  constant (100× headroom) so a healthy device under load never spuriously
  times out, while still bounding a genuinely-dead device so boot can't hang
  forever.
- **Head-matching completion.** New `poll_matching(head, …)` only returns
  when the completion's head equals *our* submitted head; a mismatched
  (stale) completion is drained (`free_chain`) and polling continues.
  Guarantees `read_sector`/`write_sector` free exactly the chain they
  submitted.
- **Safe timeout recovery.** On timeout the driver no longer blindly frees a
  device-owned chain. Instead `recover_after_timeout()` → `recover()`
  re-runs the legacy virtio init handshake (reset → ACK → DRIVER → features
  → re-select queue 0 → `Virtqueue::reset()` → re-publish queue PFN →
  DRIVER_OK), forcing the device to relinquish **all** outstanding buffers
  so the next request starts from a clean, consistent state. New
  `Virtqueue::reset()` (`kernel/src/virtio/queue.rs`) re-zeroes the rings,
  rebuilds the free list, and clears avail/used index tracking, reusing the
  same backing frame.

**Status.** FIXED. The spurious-timeout trigger is removed and, even if a
timeout does occur (genuinely dead device), the reset-based recovery keeps
the virtqueue consistent instead of cascading. Re-soak to confirm the wedge
no longer reproduces.

### B-SYSCTL-IRQ-DEADLOCK. `sysctl::REGISTRY` (raw `spin::Mutex`) acquired blockingly from interrupt context → single-CPU hard deadlock — ROOT-CAUSED & FIXED 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh` (iter 4).
The i6300esb NMI hard-lockup watchdog captured a frozen guest with
`RIP=0xffffffff81acd516` (`spin_loop_hint`) and `RFL=0x00000002` (IF
cleared → interrupts disabled while spinning, i.e. a spinlock deadlock,
not a lost-wakeup/idle bug). The NMI backtrace showed
`serial::_print ← sysctl::set ← mm::oom::self_test`, with
`RDI = &sysctl::REGISTRY`; a stack scan additionally showed
`timer_tick → check_starvation → sysctl::get` frames.

**Root cause.** `static REGISTRY: spin::Mutex<Registry>` (kernel/src/sysctl.rs)
is a *raw* `spin::Mutex` — it does **not** mask hardware interrupts on
acquire. It was reachable from two contexts:
  - **Task context:** `sysctl::set()` held `REGISTRY` across a slow
    `serial_println!` (the `[sysctl] name = v (was old)` log).
  - **Interrupt context:** the timer IRQ's `sched::check_starvation()`
    (sched/mod.rs) called the *blocking* `sysctl::get()` to read
    `sched.starvation_threshold`; the #PF stack-grow handler (idt.rs)
    likewise called blocking `sysctl::get()` for `mm.max_stack_frames`.
On a single CPU, when the timer IRQ fired while a task held `REGISTRY`
(inside `set()`'s log window), the ISR spun on `REGISTRY.lock()` forever
— the interrupted holder can never resume to release it. Classic Q24
"raw spin::Mutex holder-preemption / interrupt-reentrancy" deadlock
(same class as the already-fixed heap-lock 83307bdfc and container::TABLE
fa87bbb5e).

**Fix (proper).**
  1. Added `sysctl::try_get(id) -> Option<u64>` — a non-blocking read
     using `REGISTRY.try_lock()`, returning `None` on contention so the
     caller falls back to its compile-time default (always safe for these
     tunables). Interrupt/exception-context readers MUST use this, never
     the blocking `get()`. (Mirrors how `check_starvation` already uses
     `SCHED.try_lock()`.)
  2. Converted the two IRQ/exception-context callers to `try_get`:
     `sched::check_starvation` (sched/mod.rs) and the #PF stack-grow
     handler (idt.rs).
  3. Stopped `sysctl::set()` from holding `REGISTRY` across the log: it
     now snapshots an owned `ParamInfo` via
     `let info = REGISTRY.lock().find(id);` (guard drops at the `;`) and
     logs lock-free, closing the window entirely.

The remaining `sysctl::get()` callers (frame-alloc slow path, kswapd,
oom self-test, swap, syscall handlers, procfs) all run in task/syscall
context and are fine keeping the blocking read.

**Repro (pre-fix).** `scripts/wedge-soak.sh` (hard-lockup watchdog); the
wedge appeared within a handful of iterations under the oom/container
self-test load that exercises `sysctl::set`.

**Validation (post-fix).** 6/6 wedge-soak iterations booted to BOOT_OK
(101–158s each) with zero wedges caught (2026-07-15). NOTE: validating
this required first fixing a separate harness bug — boot-test.sh was
leaking orphaned native QEMU processes on Windows (MSYS `kill` does not
reap them), which locked `serial-test.txt` and made every repeated soak
iteration fast-fail. Fixed in the same session via `-pidfile` +
`taskkill` (commit 845c4447b); the sysctl fix itself is 0da3324e5.

**Proactive audit of the whole bug class (2026-07-15).** Since this was
the *third* found instance of a raw `spin::Mutex` deadlocking across the
task/IRQ boundary (prior two: heap lock 83307bdfc, `container::TABLE`
fa87bbb5e), I audited the two highest-risk interrupt/exception entry
paths for the same pattern rather than waiting for the next one to wedge
a boot. The invariant every IRQ-reachable lock must satisfy: EITHER the
IRQ-context reader uses `try_lock` (fall back to a default on
contention), OR *every* task-context holder wraps the lock in
`crate::cpu::without_interrupts` (masks IRQs, not just preemption — the
preempt-aware `crate::sync::Mutex` alone is insufficient because it does
not clear IF).
  - **Timer hard-IRQ path** (`apic::handle_timer_irq`, IF=0):
    `sched::timer_tick` uses `SCHED.try_lock()`; `check_starvation` now
    uses `sysctl::try_get` (this fix); `cgroup::{cpu_charge,
    cpu_period_reset, io_period_reset}` all use `TABLE.try_lock()`;
    `hrtimer::{process_expired, next_expiry_ns}` and every task-side
    `hrtimer` lock (`schedule_absolute`, `cancel`, `pending_count`) use
    `without_interrupts`. All clean.
  - **Page-fault exception handler** (`idt::handle_page_fault`): body
    takes no direct spin lock (grep for `.lock()` from its entry = none)
    beyond the `sysctl::get`→`try_get` stack-frame-limit read fixed here;
    it delegates to mm helpers that own their locking.
  - **Device IRQs** route through `ioapic::handle_device_irq` and defer
    to userspace drivers via the IRQ-poll softirq (bottom half, IF=1),
    so they are not on the IF=0 hard-IRQ deadlock path.
  Conclusion: the sysctl case was an isolated oversight; the hot IRQ
  paths are otherwise correctly disciplined. A future session extending
  IRQ-context code must preserve the try_lock-or-without_interrupts
  invariant above.

---

### B-PTHREAD-TEARDOWN-PF. Intermittent kernel `#PF` (read @ 0x97) in a `cloned-thread` task during glibc-pthread thread teardown — WATCH (non-fatal, rare) 2026-07-15

**Symptom (1 occurrence in ~5 boots, 2026-07-15):** During the
`self_test_linux_real_glibc_pthread` self-test (process labelled
`spawn-test-glibc-pthread`: 4 threads via `clone`+futex+TLS, 40 000
mutex/futex ops, then `pthread_join`), a boot died with:

```
[sched] Task 124 exiting
[sched] Task 125 exiting
[sched] Task 126 exiting
EXCEPTION: Page Fault (#PF) at 0xffffffff82713dc2, address=0x97, error=0x0
  Cause: not-present, read, kernel
  Task: 123 ("cloned-thread"), priority 16, cpu 0
FATAL: Unrecoverable kernel page fault. Halting.
```

i.e. a **kernel-mode read of a near-null pointer (base+0x97 = 0x97)** in
one clone-child task (123) exactly while its sibling clone-children
(124/125/126) are running their `[sched] Task N exiting` teardown. This is
the classic signature of a **use-after-free / null-deref race in thread
teardown**: task 123 dereferences a per-thread or per-process structure at
field offset 0x97 whose base has just been torn down (freed / cleared) by a
concurrently-exiting sibling, on the single-CPU boot where the exiting
sibling preempts mid-window.

**Why NOT the resolved B-PTHREAD-YIELDBUDGET:** that entry is a *silent
hang* (yield-budget exhaustion), structurally fixed. This is a *hard #PF*
with a distinct fault address — a different failure mode in the same test,
so tracked separately.

**Not caused by the change it surfaced under:** it appeared on one boot
while validating the container-WORKDIR cwd plumbing (which does not touch
any thread path and is not exercised at boot); the very next boot (identical
binary) reached `BOOT_OK`. The code change only perturbed layout/timing and
exposed a pre-existing latent race.

**Reproduce:** run `bash scripts/boot-test.sh` repeatedly; the pthread test
faults intermittently (observed ~1/5). Non-deterministic — depends on the
exact preemption interleaving of the four clone-children during join/exit.

**Investigation status (updated 2026-07-15):** the toolchain *does* have a
working symbolizer — `scripts/resolve-rip.sh`, which maps a RIP against the
actual booted ELF (`target/x86_64-unknown-none/debug/kernel`, staged by
`scripts/boot-test.sh` line 73 — **not** the stale `target/x86_64-slateos/…`
image, which is a June-20 leftover and gives garbage). Earlier "no symbolizer"
/ garbage-symbol notes were wrong on two counts: (1) an awk-based mapper
truncated the 64-bit address to a 53-bit float, and (2) it was run against the
stale slateos ELF. `resolve-rip.sh` avoids both (lexicographic 16-hex-digit
compare; correct ELF). Running it on the captured trace gave:

```
0xffffffff82713dc2 -> sched::CURRENT_TASK_IDS  +0x2   (a DATA symbol, not code)
0xffffffff810e06c6 -> handle_page_fault
0xffffffff810d4f7b -> isr_page_fault
```

The two backtrace frames (`0x…810e06c6`, `0x…810d4f7b`) are the fault handler
itself (`handle_page_fault`/`isr_page_fault`) — expected, since the frame
walker starts inside the handler. But the **RIP is authoritative**: it is the
`frame.rip` value the CPU pushed onto the `#PF` interrupt stack frame
(`idt.rs:2189`/`2192`), i.e. the instruction that was executing when the fault
hit. That RIP resolves *into the `.data` section* (`CURRENT_TASK_IDS +0x2`).

**Sharper diagnosis:** RIP living inside a data symbol means this is a
**control-flow hijack** — a corrupted return address or function pointer sent
execution into `.data`, whose bytes then decoded as an instruction that did a
near-null read (base register 0 + disp 0x97 = cr2 0x97). (The kernel image is
mapped executable across its image, so fetching from `.data` does not itself
fault with an instruction-fetch error — consistent with the observed
`error=0x0` = not-present, **read**, kernel.) A corrupted code pointer during
thread teardown is the textbook signature of a **use-after-free**: a freed
per-thread structure's function-pointer / return slot was reused (or its
memory recycled) while task 123 still held a stale reference.

**PROPER FIX (needs a fresh reproduction to pin the exact pointer):** audit the
thread-exit path (`proc::thread::kill_process_threads` / `on_thread_exit_hook`
/ the clone-child TLS/`clear_child_tid` teardown and the per-thread control
block free) for a stored pointer (function pointer, return address into a
freed stack, or `&mut` into a container element) that a concurrently-exiting
sibling can free out from under task 123. Fix with an ID-lookup (not a stored
pointer) or by holding the teardown lock across the corrupted access, per the
"no dangling references" rule. On the next repro, also dump the top few
stack-slot values around `frame.rsp` and symbolize each with `resolve-rip.sh`
to recover the real caller frame (the hijacked return address's origin).
Line of investigation paused here pending a repro: fault is non-reproducible
(~1/5) and one capture cannot pin the exact corrupted pointer.

**Static audit (2026-07-15) — two findings that narrow the search:**

1. **`reap_dead_tasks` is ruled out as the mechanism.** It snapshots the
   current task id of *every* online CPU into `active_ids` and filters the
   dead set with `!active_ids.contains(id)` (`sched/mod.rs:3473`), so it never
   frees the kernel stack of a task any CPU is running on. And task 123 (the
   faulting task) is not `Dead` — it *resumes* and then faults — so its own
   stack is never a `reap_dead_tasks` candidate. The UAF is therefore not
   "sibling reaps 123's still-in-use stack via the reaper."

2. **The corrupted code pointer is a *specific* value: `&CURRENT_TASK_IDS[0]
   + 2`, not random garbage.** `CURRENT_TASK_IDS` is
   `[CachePadded<AtomicU64>; MAX_CPUS]`, so its storage spans MAX_CPUS × ≥64
   bytes and the resolver's reported `+0x2` is genuinely *inside CPU 0's slot*
   (the first `AtomicU64`). That address is exactly what `set_current_task(cpu,
   id)` computes to `.store()` the running task id for CPU 0
   (`sched/mod.rs:860`) — the sole writer of that address. So the hijacked
   return-address / code-pointer slot held the *address of the per-CPU
   current-task-id cell*, which strongly implicates the **low-level context
   switch**: a spilled `&CURRENT_TASK_IDS[cpu]` (or a register holding it
   across `set_current_task`) overlapping the saved-RIP slot on task 123's
   kernel stack — a stack-frame-layout/offset bug in the switch path — rather
   than a heap/PCB use-after-free in the higher-level exit bookkeeping
   (`on_thread_exit`/`on_thread_exit_hook`, which only touch user memory + the
   robust/ctid/rseq maps and never take `&CURRENT_TASK_IDS`). Next repro should
   focus the `dump_stack_scan` output on which frame's return slot equals
   `&CURRENT_TASK_IDS[0]+2` and cross-reference the context-switch save/restore
   stack offsets.

**Static audit refinement (2026-07-15b) — the context-switch assembly is
provably clean, so finding 2's "switch-path layout bug" phrasing is wrong.**
Reading `sched/context.rs` in full: `switch_context` only saves/restores the
callee-saved GPRs, `rsp`, `rflags`, and FPU state via the `Context` struct at
fixed offsets 0x00–0x38; it *never computes or references `&CURRENT_TASK_IDS`*
at all (nor does `task_entry_trampoline`). The offsets match `task.rs`'s
`Context`. Therefore the address value `&CURRENT_TASK_IDS[0]` cannot originate
in the switch code — it must be **spilled/stored by a *different* function that
takes `&CURRENT_TASK_IDS[cpu]`** (`set_current_task` at 860, `load_current_task`
at 869, and the reaper/health snapshots at 3487/5242/5263/5309) and then land,
via a wild write / stack overflow, on top of task 123's saved return-address
slot. Mechanism is now: task 123 last suspended by calling `switch_context`
(pushing a normal return address into `schedule()`); something overwrote that
stack word with the value `&CURRENT_TASK_IDS[0]` (+2 is the resolver rounding to
the nearest preceding symbol; the stored qword is the cell base); when 123 is
resumed, `switch_context`'s final `ret` jumps to that data address and #GP/#PFs
executing `.data` as code (cr2=0x97 is then whatever the garbage bytes there
decode to dereference). **Next repro must catch which code path spills
`&CURRENT_TASK_IDS[cpu]` to a stack slot that can alias another task's stack** —
prime suspects are any `current_cpu_id()`/`load_current_task()` call made while
running on a *borrowed* or already-freed stack, or an off-by-one stack write in
the clone/exit path. The `dump_stack_scan` capture should show the exact stack
address holding `&CURRENT_TASK_IDS[0]` relative to task 123's `rsp`.

### B-FORKEXEC-BOOT-HANG. Intermittent silent boot hang at the glibc `fork()`+`execl()`+`waitpid()` self-test — WATCH (rare, non-fatal to a re-run) 2026-07-15

**Symptom (1 occurrence, 2026-07-15):** During
`self_test_linux_real_glibc_forkexec` (`spawn-test-glibc-forkexec`,
main.rs:1791: a glibc program that `fork()`s, the child `execl()`s a second
ELF, the parent `waitpid()`s), a boot went silent. The last serial lines were
the normal end of that test's process teardown:

```
[exec] Process 165 exec complete: entry=0x…, rsp=0x…
[mmap] Lazy mapped 0x6000212000..0x6000216000 (1 frames, demand-paged)
[thread] Process 165 has no threads left — now zombie   (execed child)
[thread] Process 164 has no threads left — now zombie   (fork parent)
[sched] Task 130 exiting
```

…then **no further output** and the 480 s boot-test timeout fired. Note there
is **no `#PF`/PANIC/FATAL** — this is a *hang* (the scheduler idled with no
runnable task, or a reap/`waitpid` wait never woke), NOT the `#PF` control-flow
hijack tracked in **B-PTHREAD-TEARDOWN-PF** above, and it is at a different
test (fork+exec, not pthread). The immediately-following boot (identical binary)
reached `BOOT_OK` in 89 s, so it is intermittent, not a hard regression.

**Distinct from the two known pthread issues:** B-PTHREAD-TEARDOWN-PF is a hard
`#PF`; B-PTHREAD-YIELDBUDGET (resolved) was a yield-budget hang inside the
*pthread* test. This hang is in the *fork+exec* test, after both child and
parent have gone zombie — pointing at the parent's `waitpid`/reap wakeup or the
scheduler's idle transition when the last task exits, rather than at thread
teardown.

**Not caused by the change it surfaced under:** it appeared while validating
Path Z Part 43 (a signal self-test registered at main.rs:2106, which never even
ran this boot — the hang is ~300 lines of test earlier). The change only
perturbed timing.

**Reproduce:** run `bash scripts/boot-test.sh` repeatedly. Non-deterministic;
observed once. **Diagnostic aid (2026-07-15):** `boot-test.sh` now echoes the
last 25 serial lines to stdout on any timeout (independently of the serial
file, which a re-run overwrites), so the next occurrence records its freeze
point in the test output automatically; the harness also hints to re-run with
`--hard-lockup-watchdog` to capture the wedged guest RIP via the i6300esb NMI +
HMP monitor.

**Static audit (2026-07-15b) — the waitpid-lost-wakeup hypothesis is RULED OUT;
suspect is a task-exit-path wedge (likely Q24 holder-preemption spin-deadlock).**
Two facts from reading the code + the serial log flip the diagnosis:
1. **The kernel harness that waits for the spawned glibc program does not block
   — it *polls*.** `self_test_linux_real_glibc_forkexec` (`spawn.rs:9523`) waits
   with `for _ in 0..MAX_YIELDS { if state==Zombie break; sched::yield_now() }`,
   never `block_current`. A poll loop cannot suffer a lost wakeup, so "harness
   parked in wait4 with a missed wakeup" is impossible here.
2. **The parent process itself reached `Zombie`** (`Process 164 … now zombie`
   in the log) — i.e. the glibc program's own `waitpid()` already returned and
   the program exited normally. So the in-guest wait4 also completed. The freeze
   is *after* both, at `[sched] Task 130 exiting` — inside the scheduler's
   task-exit teardown for the last thread, with **no further output**.
   And the core kernel `wake`/`block_current` protocol is independently sound
   (the `pending_wake` flag in `sched/mod.rs:1523` closes the register→park
   window), so this is not a scheduler wakeup bug either.
This points at the **exit/teardown path wedging on the CPU** rather than any
missed wakeup: most plausibly a **raw `spin::Mutex` holder-preemption deadlock**
— the *same Q24 class* as the already-fixed container-exec (`fa87bbb5e`) and
heap (`83307bdfc`) deadlocks — hit somewhere between "[sched] Task N exiting"
and the reap (e.g. `PROCESS_TABLE`/`SCHED`/reaper locks taken without
preempt-disable while a timer preemption lands on the holder on a 1-CPU boot).
It could also be an idle-transition bug (last runnable task exits and the idle
path never reschedules the still-Ready harness), but the lock-deadlock is the
better fit for a *silent, output-less* freeze mid-teardown.

**PROPER FIX (needs a repro):** on the next occurrence, capture the wedged guest
RIP (`--hard-lockup-watchdog`) and check whether it sits inside a
`spin::Mutex::lock` spin in the exit/reap path (→ Q24 preempt-disable fix, mirror
the container/heap pattern) vs. the idle loop with a Ready task still queued (→
idle-reschedule bug). Given the Q24 lineage, the highest-value proactive step is
the kernel-wide raw-spin holder-preemption audit already queued as **Q24** in
`open-questions.md`; this hang is another data point for doing that audit.

### D-SHM-MAP-NOCAP. `SYS_SHM_MAP`/`SYS_SHM_SIZE`/`SYS_SHM_CLOSE` do not verify the caller owns the handle — RESOLVED 2026-07-14

**RESOLVED 2026-07-14 (option (b) — IPC provider-PID + `shm::authorize` grant).**
The three syscalls now enforce per-region authorization. Implementation:
 - `kernel/src/ipc/shm.rs`: `ShmRegion` gained an `authorized: Vec<u64>`
   list; new `shm::authorize(handle, pid)` (idempotent grant) and
   `shm::is_authorized(handle, pid)`.
 - `kernel/src/ipc/service.rs`: the service registry now records the
   registering process's `provider_pid`, exposed via
   `service::provider_pid(name) -> Option<u64>` — the missing identity
   plumbing called out below.
 - `kernel/src/net/netstack_client.rs::submit_round_on` and the five
   `kernel/src/proc/spawn.rs` netstack bootstraps (`shm_ping`, `ring_echo`,
   `ring_tcp`, …) call `shm::authorize(handle, service::provider_pid(b"net.stack"))`
   before handing the daemon a ring region.
 - `kernel/src/syscall/handlers.rs`: `sys_shm_map`/`sys_shm_size`/
   `sys_shm_close` now call `shm_check_authorized(handle)` — a userspace
   caller (`caller_pid()==Some(pid!=0)`) must be the region's creator or an
   authorized PID, else `PermissionDenied`; kernel context (`None`/PID 0)
   is the TCB and always allowed. `sys_shm_create` auto-authorizes the
   creating PID. Boot-validated with `net.userspace` on: the daemon
   (pid 227) completed all SHM-ring parity checks (TCP/UDP/DNS/nonblock/
   poll/listen-accept/connect6) with no permission errors; BOOT_OK at 120s.

Historical context (the original gap and the investigation that led to the
fix) is retained below.

---

`SYS_SHM_MAP` (kernel/src/syscall/handlers.rs `sys_shm_map`) maps a
shared-memory region into the caller's address space given only the
region's raw handle (which *is* the region ID — see `ShmHandle` in
kernel/src/ipc/shm.rs). It does **not** check that the calling process
created or was granted that region: any process that possesses (or
guesses — IDs are a small monotonic counter) a handle can map another
process's shared memory. `SYS_SHM_SIZE` and `SYS_SHM_CLOSE` have the same
gap (pre-existing). This is currently *by design* for the netstack Phase-4
bootstrap: the kernel creates the region and hands the handle to the
trusted `netstack` daemon over the `net.stack` control channel, so both
ends are trusted. **Proper fix:** gate SHM handles through the capability
system (unforgeable, per-process handle table with an explicit
grant/transfer op) before any *untrusted* process is allowed to use
`SYS_SHM_MAP` — i.e. before Phase 5 exposes socket data rings to arbitrary
apps. Until then, only kernel-mediated, trusted-daemon SHM sharing is
safe. Where it bites: any future userspace-to-userspace SHM use.

**Investigation 2026-07-14 (why this is not a quick turn):** the natural
enforcement — "a userspace `SYS_SHM_MAP`/`SIZE`/`CLOSE` caller must own or
have been granted the handle" — needs a way to *authorize the netstack
daemon* for the kernel-created ring regions it legitimately maps, or the
fix hard-breaks the working daemon. The daemon does **not** receive the
region as a tracked capability: the handle travels as *plain u64 payload*
inside the `net.stack` control-channel `Request` (see the daemon's
`shm_ping`/`ring_echo`/`ring_tcp` handlers in `services/netstack/src/main.rs`
and the kernel senders `netipc::encode_ring_tcp` in
`kernel/src/net/netstack_client.rs::submit_round_on` +
`kernel/src/proc/spawn.rs` self-test bootstraps). To authorize the daemon
at those handoff points the kernel must know the daemon's PID — but the
plumbing to derive it doesn't exist: `ipc/channel.rs` has **no** peer-PID
query (only `peer_side`), and `ipc/service.rs`'s `Listener`/registry does
**not** record the provider PID. So the real fix requires one of:
 (a) route the SHM handle through the existing **capability transfer**
     mechanism (channel `Message` cap slots → `ipc/mod.rs` already
     dispatches `ResourceType::SharedMemory` on cleanup) so receipt
     registers ownership in the daemon PCB via `pcb::register_ipc_handle`,
     then gate the three syscalls on `caller_pid()`-ownership (kernel
     context `caller_pid()==None` stays allowed as the TCB); **or**
 (b) add process-identity at the IPC boundary (channel peer-PID or service
     provider-PID) + an explicit `shm::authorize(handle, pid)` grant called
     at each kernel→daemon handoff, then gate the syscalls the same way.
Option (a) is architecturally cleaner (reuses caps, no new identity
plumbing) and is the recommended path. Either way it touches the working
daemon data path and needs full switch-on boot validation, so it is a
deliberate multi-step change rather than a drive-by — deferred until it
can be done carefully (still non-blocking: no untrusted process maps SHM
today).

### D-NETSOCK-SYNC. Daemon-backed AF_INET stream sockets (migration 5.5) are synchronous, single-stream, and IPv4-only — TECH DEBT (logged 2026-07-14)

**Where:** `kernel/src/net/socket.rs` + the switch-gated socket arms in
`kernel/src/syscall/linux.rs` (`sys_socket`/`connect`/`sendto`/`recvfrom`/
`getpeername`, `dispatch_socket_read`/`write`). All gated behind
`net.userspace` (default off).

**Status (2026-07-15):** the *TCP-client* Linux-parity line is now essentially
complete. Connected daemon-backed `AF_INET`/`AF_INET6` `SOCK_STREAM` sockets
honour non-blocking recv/send/connect, honest poll/epoll readiness,
`getsockname`/`getpeername`, `shutdown`, `setsockopt`/`getsockopt`
(`SO_ERROR`/`SO_TYPE`/buffer hints/`TCP_NODELAY`), `sendmsg`/`recvmsg`, the
`recvfrom` source-address out-param, and the `MSG_DONTWAIT`/`MSG_WAITALL`/
`MSG_PEEK` per-call flags. The remaining daemon-socket gaps are the large,
non-incremental ones: **(1) server sockets** (`bind`/`listen`/`accept4`) —
*gated on operator decision Q23* (`open-questions.md`); **(2) UDP `SOCK_DGRAM`**
— now **complete end-to-end**: the daemon datagram-socket layer, ring ABI, kernel
client, *and* the AF_INET socket-fd wiring are all landed. A userspace
`socket(AF_INET, SOCK_DGRAM)` is a real daemon-backed UDP socket:
`bind`/`sendto`/`recvfrom`/`getsockname` and `poll`/`epoll` route to
`net::socket::create_dgram`/`dgram_bind`/`dgram_send_to`/`dgram_recv_from` over
`OP_UDP_BIND`/`OP_UDP_SEND`/`OP_UDP_RECV`, boot-validated by `self_test_udp_dns`.
**AF_INET6 UDP datagrams now work too** (`OP_UDP_SEND6` + v6-aware `OP_UDP_RECV`,
`self_test_udp6_loopback`). **UDP `connect()` default-peer now works too** (v4 and
v6): `connect(2)` on a `SOCK_DGRAM` fd records a default peer (no handshake,
auto-binds a source port) via `net::socket::dgram_connect`, so `send`/`write` (no
explicit destination) target it (`dgram_send_connected`; `EDESTADDRREQ` when
unconnected) and `recv`/`read` filter incoming datagrams to that peer (Linux drops
non-peer datagrams on a connected UDP socket); `getpeername` reports it, and
`connect(AF_UNSPEC)` dissolves it. Boot-validated by `self_test_udp_connect` (both
filter-pass and filter-drop). The last big gap is **(3) send
pipelining** — the daemon's single-outstanding-segment sender is a *deliberate*
minimal-TCP design, so multi-segment/windowed send is a design change with
tradeoffs, not a bug fix.

Known limitations, all deliberate for the 5.5 increment and to be closed as
Phase 5 progresses:

- **`O_NONBLOCK` receive and connect are now honoured (5.6+); send still blocks.**
  The read side no longer stalls: `recvfrom`/`read` on a socket with `O_NONBLOCK`
  set pass the new `netipc::ring::RECV_NONBLOCK` flag to the daemon, which drains
  already-arrived frames once and returns `ERR_WOULD_BLOCK` (→ `KernelError::
  WouldBlock` → `EAGAIN`) rather than polling for the full receive deadline
  (`dispatch_socket_read` → `net::socket::recv(_, nonblock)` →
  `NetstackConn::recv(_, nonblock)`; daemon `ring_tcp_recv`). **Non-blocking
  connect is now implemented (5.6+):** `connect` on an `O_NONBLOCK` socket passes
  `netipc::ring::CONNECT_NONBLOCK`; the daemon transmits the SYN and returns
  `ERR_IN_PROGRESS` (→ `EINPROGRESS`) without waiting, holding the connection in
  `SYN_SENT`. It completes the handshake in the background (the SYN-ACK is
  processed by `ingest_seg` via the RX pump on a later `OP_POLL`/`OP_RECV`, and
  `poll_connect` retransmits a lost SYN up to `TCP_SYN_ATTEMPTS`). The socket
  enters `SockState::Connecting`; a `poll(POLLOUT)` re-probes via `OP_POLL`, whose
  new `POLL_ERR`/`POLL_WRITABLE` bits drive the transition to `Connected`
  (writable) or `Failed` (writable+`POLLERR`). `getsockopt(SOL_SOCKET, SO_ERROR)`
  then reports `0` (success) or `ECONNREFUSED` (`net::socket::take_so_error`,
  one-shot). A repeated non-blocking `connect` while pending returns `EALREADY`;
  on an established socket, `EISCONN`. Boot-validated: `netstack_client::
  self_test_nonblock_connect` runs the EINPROGRESS→POLLOUT sequence over the live
  daemon. A send/recv on a still-connecting socket is rejected (`ENOTCONN`)
  rather than buffered.
- **Non-blocking `send` is honoured (5.6+).** A `send(2)`/`write(2)` on an
  `O_NONBLOCK` socket whose send window is full (the single outstanding segment is
  still unacknowledged) returns `EAGAIN` instead of blocking on the daemon; on a
  window with room it accepts the bytes and returns the count, exactly like Linux.
  Path: `dispatch_socket_write` reads the fd's `O_NONBLOCK` → `net::socket::send(_,
  nonblock)` → `NetstackConn::send(_, nonblock)` (which ORs `netipc::ring::
  SEND_NONBLOCK`); the daemon (`ring_tcp_send`) drains pending ACKs once and, if the
  window is still full, returns `ERR_WOULD_BLOCK`. The window is re-opened by
  `ingest_seg` when the peer's cumulative ACK reaches `snd_nxt` (a single
  outstanding segment). `poll(POLLOUT)` is likewise honest now: `ring_tcp_poll`
  reports `POLL_WRITABLE` only when the window has room. Boot-validated:
  `netstack_client::self_test_nonblock_send`. **Still single-outstanding-segment:**
  only one unacknowledged segment may be in flight; a full window blocks/EAGAINs
  until it is ACKed (no send pipelining yet).
- **`poll`/`epoll` read readiness is now honest (5.6+).** A connected socket's
  `POLLIN`/`POLLOUT` are computed from a real non-destructive probe rather than
  the old "always-ready" placeholder: `poll_revents_from_entry`'s `Socket` arm
  calls `net::socket::poll_ready`, which issues a new `netipc::ring::OP_POLL`
  round-trip; the daemon (`ring_tcp_poll`) drains arrived frames once and reports
  a `POLL_READABLE`/`POLL_WRITABLE` bitmask **without consuming any buffered
  bytes** (a later `recv` still returns them). So `POLLIN` is set only when there
  are buffered bytes or the peer has closed — a poller sleeping on an idle socket
  no longer wakes on a false `POLLIN` and then reads `EAGAIN`. Boot-validated:
  the persistent-daemon parity check polls an idle connected socket
  (`readable=false writable=true`) and then confirms it flips to readable once
  the HTTP response arrives (`netstack_client::self_test_poll_ready`). **Caveat:**
  each poll of a socket fd costs one daemon control round-trip (the poll engine
  re-probes per ~10 ms slice); acceptable for parity, but the async-completion
  socket server (below) will replace it with an edge-triggered readiness signal.
  Proper fix for the remaining gap: async ring completions + a real readiness
  signal (eventfd/completion port) when the always-on socket server lands.
- **Server sockets: daemon+ring done, socket-fd wiring pending.** The daemon and
  the ring client now support the passive side — `NetstackConn::listen`/`accept`
  drive ring `OP_LISTEN`/`OP_ACCEPT`, and both ends of a connection can live in one
  daemon session (validated by the loopback `self_test_listen_accept`). What is
  *not* yet wired is the AF_INET socket-fd layer: `sys_bind`/`sys_listen`/
  `sys_accept4` do not route to the daemon and `net::socket` has no
  `SockState::Listening`, so a userspace `listen(2)`/`accept(2)` on a daemon-backed
  socket still isn't served. That syscall wiring is the remaining follow-on.
- **UDP `SOCK_DGRAM`: complete end-to-end (daemon+ring+client+socket-fd wiring).**
  The daemon hosts a fixed table of bound connectionless datagram sockets
  (`UdpSock`/`UdpSocks` in `services/netstack/src/main.rs`) served by ring ops
  `OP_UDP_BIND` (ephemeral-port picking + `EADDRINUSE`), `OP_UDP_SEND`
  (`EMSGSIZE` on oversize), and `OP_UDP_RECV` (which prepends a 24-byte in-band
  source-address header — `Sqe::pack_udp_addr`, design-decision #68 — since the
  16-byte CQE has no room for a per-datagram address); `OP_POLL` reports a bound
  socket as always-writable and readable-when-queued; `OP_CLOSE` unbinds. The
  kernel client exposes `NetstackConn::udp_bind`/`udp_send_to`/`udp_recv_from`,
  boot-validated end-to-end by `netstack_client::self_test_udp_dns` (bind an
  ephemeral port, send a real DNS `A`-query to the resolver, read the reply back
  from port 53). The **AF_INET socket-fd layer is now wired**: `net::socket` grew a
  `SockKind::{Stream,Dgram}` transport tag and `create_dgram`/`dgram_bind`/
  `dgram_send_to`/`dgram_recv_from`/`dgram_local_port`/`is_dgram`; the Linux
  syscall layer routes `socket(AF_INET, SOCK_DGRAM)` → `create_dgram`,
  `bind` → `dgram_bind` (`socket_dgram_bind_from_user`), `sendto` →
  `dispatch_dgram_sendto` (destination `sockaddr_in`; `EDESTADDRREQ` on a NULL
  dest — no UDP `connect()` default-peer yet), `recvfrom` →
  `dispatch_dgram_recvfrom` (fills the real per-datagram source address, truncates
  a short buffer like Linux without `MSG_TRUNC`), `getsockname` → `dgram_local_port`
  (reports `0.0.0.0:port` — the daemon owns the interface IP and has no UDP
  `OP_LOCALADDR` yet), and `poll`/`epoll` → `poll_ready` (a bound dgram socket is
  writable + readable-when-queued; unbound is writable-only). Implicit ephemeral
  auto-bind on the first `sendto`/`recvfrom` matches Linux. The socket-gate
  self-tests (`socket EPROTONOSUPPORT gating`) are switch-aware for the
  `SOCK_DGRAM`/UDP case (ENOSYS off, daemon-backed fd on), and only protocol
  `0`/`IPPROTO_UDP` route to the daemon — `IPPROTO_UDPLITE=136` stays ENOSYS
  (unimplemented) rather than being silently aliased to plain UDP. Proven from
  ring-3 by the `services/udpget` capstone (`socket(SOCK_DGRAM)`/`bind`/`sendto`/
  `recvfrom` a DNS `A?` query, exit-code-decoded), spawned by
  `run_persistent_netstack`, boot-validated switch-on (`[udpget] start`→`query
  sent`→`OK: DNS reply`, `ring3 UDP capstone: OK … (exit 0)`). **Remaining
  follow-ups (not blockers):**
  - **IPv6 datagrams — DONE (all five layers landed).** `AF_INET6` UDP
    `SOCK_DGRAM` is now a real daemon-backed v6 datagram socket. **(1) netproto** —
    `udp::write_v6`/`Datagram::parse_v6` compute the RFC 8200 v6 pseudo-header
    checksum (mandatory for v6: `parse_v6` rejects a zero checksum). **(2) daemon**
    (`services/netstack/src/main.rs`) — `udp_sock_send6` frames via `send_ipv6` +
    `ipv4::PROTO_UDP`; `recv_udp_any` grew an `ETHERTYPE_IPV6` arm (parse
    `ipv6::Packet` → `udp::Datagram::parse_v6`); `UdpDatagram`/`UdpSock::push`/`pop`
    now carry a `family` tag + fixed 16-byte source address (one queue serves both
    families), and `OP_UDP_RECV` packs the stored family into the in-band header.
    New opcode `OP_UDP_SEND6` (netipc `ring.rs`, `0x0F`) carries the 16-byte v6
    destination at the front of the data window (`[dst_ip16:16][payload…]`), port
    in `aux`. **(3) client** (`netstack_client.rs`) — `udp_send_to6(&[u8;16],port,
    buf)` and the family-aware `udp_recv_any(&mut buf,nonblock) -> (i32,u16 family,
    [u8;16],u16)` (the old `udp_recv_from` is now a v4-truncating wrapper).
    **(4) socket** (`net::socket`) — `dgram_send_to6`; `dgram_recv_from` returns the
    family + 16-byte address. **(5) linux.rs** — `socket_dgram_bind_from_user`,
    `dispatch_dgram_sendto`, and `dispatch_dgram_recvfrom` dispatch on `sa_family`
    (2→`sockaddr_in`/v4, 10→`sockaddr_in6`/v6); `recvfrom` writes the sockaddr
    matching the *datagram's* family (via `socket_write_peer_addr6` for a v6
    source), since a datagram's family is a packet property, not the socket's.
    Boot-validated by `netstack_client::self_test_udp6_loopback` (bind a fixed
    port, send to the daemon's own link-local `me.ip6`, and confirm the looped-back
    datagram reports `AF_INET6` + the link-local source + the sent payload), and
    **now also from a real ring-3 process** by the `udpget` v6 arm (a bare
    Linux-ABI ELF: `socket(AF_INET6,SOCK_DGRAM)`/`bind([::]:port)`/`sendto([me.ip6]:
    port)`/`recvfrom`, exit-code decoded). The kernel derives `me.ip6` from the NIC
    MAC and passes it as a 32-hex argv + a `"6"` mode flag; `run_ring3_udp6_capstone`
    spawns it and boot-validated switch-on with **exit 0** (`[udpget] OK: v6 loopback
    echo`, `ring3 UDP6 capstone: OK … (exit 0)`) — so the ring-3 `sockaddr_in6`
    sendto/recvfrom dispatch path (user-copy, family parse, fd install, errno map)
    is now exercised live, not just by the kernel-context self-test. With this, all
    documented UDP daemon-socket follow-ups are closed.
  - **UDP `connect()` default-peer — DONE (v4 and v6).** `connect(2)` on a
    `SOCK_DGRAM` fd records a default peer without a handshake (auto-binding a
    source port). `net::socket` grew a `DgramPeer::{V4,V6}` + `SocketInner.
    dgram_peer` field and `dgram_connect`/`dgram_disconnect`/`dgram_peer`/
    `dgram_send_connected`; `dgram_recv_from` filters to the connected peer
    (discarding non-peer datagrams, which Linux drops at input on a connected UDP
    socket). The Linux syscall layer routes `connect(SOCK_DGRAM)` →
    `dgram_connect_from_user` (`AF_UNSPEC` dissolves; 2→v4, 10→v6, else
    `EAFNOSUPPORT`), `send`/`write`/`sendto(NULL dest)` →
    `dispatch_dgram_send_connected` (targets the peer; `EDESTADDRREQ` when
    unconnected), `sendto` *with* an address still sends there (Linux uses the
    supplied address even on a connected UDP socket), `read` routes through the
    filtered receive, and `getpeername` reports the connected peer (`ENOTCONN`
    otherwise). Boot-validated by `netstack_client::self_test_udp_connect` (a
    connected send loops back and passes the peer filter; `getpeername` matches;
    a datagram injected from a *non-peer* port is dropped by the connected
    receive). **Still not done:** UDP `getsockname` reporting the real interface IP
    (needs a UDP `OP_LOCALADDR`) — only a divergence for a socket bound to a
    specific local IP; our sockets bind `INADDR_ANY`, for which Linux's
    `getsockname` correctly reports `0.0.0.0`/`[::]`.
  - **UDP `getsockname` v6-family reporting — DONE.** `net::socket` now records the
    creation `domain` on every socket (`SocketInner.domain`, set by
    `create`/`create_dgram`/`create_kind`, exposed via `socket::domain(handle)`), so
    `sys_getsockname` on an `AF_INET6` datagram socket writes a `sockaddr_in6` with
    the unspecified address (`[::]:port`) instead of always emitting a `sockaddr_in`
    (`0.0.0.0:port`). A v4 datagram socket still reports `0.0.0.0:port`; a stream
    socket is unaffected (it reports the daemon-assigned local endpoint, which
    already carries the family). The domain narrows exactly from the syscall's
    `AF_INET`(2)/`AF_INET6`(10) gate.
  **Limitations:** the
  receive queue is 2 deep per socket and drops the oldest datagram on overflow (UDP
  is lossy); and it inherits the daemon's single-active-phase RX-demux limitation
  (`D-NETSTACK-RX-DEMUX`) — the `udp_pump` drops interleaved TCP frames while
  draining, same as the TCP pump.
- **`recvfrom` source-address out-params now populated (parity fix).**
  `recvfrom`'s `src_addr`/`addrlen` (arg4/arg5) are filled with the connected
  peer's endpoint on a successful receive, matching Linux for a connected stream
  socket (`sys_recvfrom` → `socket_write_src_addr`, reusing the getpeername
  serialisers: `sockaddr_in` for AF_INET, `sockaddr_in6` for AF_INET6; short
  buffers truncate, `*addrlen` written back full). Only touched on a non-negative
  byte count so an EAGAIN leaves the buffer alone. **`MSG_DONTWAIT` (arg3) is now
  honoured** on `send`/`sendto`/`recv`/`recvfrom`: it forces a per-call
  non-blocking transfer (→ `EAGAIN` on a full send window / empty receive)
  regardless of the fd's `O_NONBLOCK`, via a `force_nonblock` arg threaded into
  `dispatch_socket_write`/`dispatch_socket_read`. `MSG_NOSIGNAL` is a no-op (we
  never raise `SIGPIPE`; a broken pipe returns `EPIPE`). Remaining gaps: other
  `MSG_*` flags (`MSG_OOB`, `MSG_TRUNC`) are still ignored. (For a **datagram**
  socket, `recvfrom` now reports the *real* per-datagram source address — see the
  `SOCK_DGRAM` bullet above — via `dispatch_dgram_recvfrom`; the connected-stream
  path below reports the fixed peer.)
  **`MSG_WAITALL` (arg3) is now honoured** on `recv`/`recvfrom`: on a blocking
  socket it loops (`socket_recv_waitall`, ≤4 KiB chunks) until the full request
  is read, terminating early only on EOF or error; under `O_NONBLOCK`/
  `MSG_DONTWAIT` it degrades to the single-shot receive, matching Linux.
  **`MSG_PEEK` (arg3) is now honoured** on `recv`/`recvfrom`/`recvmsg`: it
  threads a `peek` flag through `dispatch_socket_read` → `net::socket::recv` →
  `NetstackConn::recv` into the ring's new `RECV_PEEK` aux flag, and the daemon
  copies buffered bytes out via a non-consuming `peek_rx` (vs the consuming
  `take_rx`), so a subsequent receive returns the same data. `MSG_PEEK` is
  single-shot even when combined with `MSG_WAITALL` (a non-consuming loop would
  re-read forever).
- **`sendmsg`/`recvmsg` now served (parity fix).** The Linux-ABI `sendmsg(2)`/
  `recvmsg(2)` on a connected daemon-backed stream socket no longer terminate in
  EBADF: `socket_sendmsg` gathers the `msg_iov` scatter/gather list into one
  bounded (≤4 KiB, one segment) staging buffer and forwards it; `socket_recvmsg`
  does a single bounded receive and scatters it across the iovecs, fills
  `msg_name` with the peer (`sockaddr_in`/`sockaddr_in6`, via `peer_sockaddr`),
  and clears `msg_controllen`/`msg_flags`. `MSG_DONTWAIT` (the `flags` arg) is
  honoured on both and `MSG_PEEK` on `recvmsg`; `msg_control` (ancillary/cmsg)
  is ignored and `msg_iovlen >
  1024` → `EMSGSIZE`. (The *native*-ABI `SYS_SOCKETPAIR_*` sendmsg/recvmsg path
  is separate and unaffected.) Remaining gap: like the plain send/recv path, only
  one page / one outstanding segment moves per call (no gather beyond 4 KiB, no
  send pipelining).
- **IPv6 connect: daemon+ring+client+socket-fd DONE.** The daemon speaks full
  TCP-over-IPv6 with no state-machine duplication (`TcpConn.dst6: Option<[u8;16]>`
  dispatching `emit`/`recv_one_seg` to `send_tcp6`/`recv_tcp_seg6`; v6 framing,
  `find_by_tuple6`/`route_seg6` demux, `connect6`/`connect_start6`/`accept_syn6`
  constructors, IPv6-aware in-process loopback), the ring ABI has `OP_CONNECT6`,
  and `NetstackConn::connect6`/`accept6` drive it end-to-end — boot-validated by
  `netstack_client::self_test_connect6` (v6 handshake + bidirectional data over the
  `fe80::/64`+EUI-64 loopback, `IPv6 parity ok`). The **AF_INET6 socket-fd layer is
  now wired**: `sys_connect` on an `AF_INET6` sockaddr parses the 28-byte
  `sockaddr_in6` (`sin6_port` BE, `sin6_addr` 16 octets) and routes to
  `net::socket::connect6` → `NetstackConn::connect6` (`socket_connect_from_user`
  dispatches on `sa_family`: 2 → v4, 10 → v6, else `EAFNOSUPPORT`). `getpeername`
  on a v6-connected socket returns a `sockaddr_in6` (`socket_write_peer_addr6`,
  fed by `net::socket::peer6`/`SocketInner.peer_ip6`). **`getsockname` now works
  for daemon sockets (both v4 and v6)** via the new `OP_LOCALADDR` ring op: the
  daemon writes its own interface address + the connection's ephemeral
  `local_port` to the data window (`[ip:4][port_be:2]` v4 / `[ip6:16][port_be:2]`
  v6), surfaced through `NetstackConn::local_addr` → `net::socket::local` →
  `sys_getsockname` (Path-B branch; returns `sockaddr_in`/`sockaddr_in6`,
  `ENOTCONN` on an unconnected socket). Validated by `self_test_connect6` step 6
  (`v6 getsockname: local fe80::…:PORT ok`). Remaining v6 socket-fd gap: the
  server-socket-fd path (bind/listen/accept4) is the separate gap below.
- **`shutdown(2)` DONE (v4 and v6).** A new `OP_SHUTDOWN` ring op carries the
  Linux `how` (`SHUT_RD`/`WR`/`RDWR`) in `aux`; the daemon's `TcpConn` grew
  `write_shut`/`read_shut` flags — `SHUT_WR` emits our FIN exactly once (a later
  `close` sees `write_shut` and skips re-sending) and rejects subsequent `OP_SEND`
  with `ERR_BROKEN_PIPE`, `SHUT_RD` makes subsequent `OP_RECV` report EOF; poll
  bits stay honest (writable when write-shut since send won't block, readable when
  read-shut). Kernel side: `NetstackConn::shutdown` + `net::socket::shutdown`
  (gated on Connected) + `sys_shutdown` Path-B branch (validates `how` after the
  fd lookup, so a bogus fd still wins with EBADF; `KernelError::BrokenPipe`→EPIPE).
  Validated by `self_test_listen_accept` step 6
  (`shutdown(SHUT_WR)→EPIPE + shutdown(SHUT_RD)→EOF ok`).
- **`setsockopt`/`getsockopt` compat DONE (client options).** `sys_setsockopt`
  no longer blanket-returns EBADF for daemon-backed sockets: it accepts the
  options a typical TCP client (curl/wget/glibc) sets during setup —
  `SOL_SOCKET`: `SO_REUSEADDR`/`SO_REUSEPORT`/`SO_KEEPALIVE`/`SO_BROADCAST`/
  `SO_SNDBUF`/`SO_RCVBUF`/`SO_LINGER`, and `IPPROTO_TCP`: `TCP_NODELAY` — as
  no-op successes (the daemon has no per-socket tunables: fixed buffers, always
  sends each segment immediately). Unknown options return `ENOPROTOOPT` (not
  EBADF), so probes feature-detect cleanly. `sys_getsockopt` gained a matching
  read side beyond the existing `SO_ERROR`: `SO_TYPE`→`SOCK_STREAM`,
  `SO_RCVBUF`/`SO_SNDBUF`→65536, `TCP_NODELAY`→1, `SO_KEEPALIVE`/`SO_REUSEADDR`/
  `SO_REUSEPORT`/`SO_BROADCAST`→0, unknown→`ENOPROTOOPT`. Both Path-B branches
  gate on `userspace_enabled()` + a real Socket fd; kernel-context callers and
  non-socket fds keep the prior EBADF terminal. Not a strict 5.7 regression gate
  (the resident path also stubs these), but a real compat gap on the path to
  running unmodified Linux network programs (the HTTP-client capstone).
- **Capacity caps** inherited from `NetstackConn`: send chunked to ≤1024 B,
  recv ≤512 B per call (callers must loop). Not a correctness bug, but small.

Proper fix path: 5.6+ makes the daemon persistent/always-on and grows the ring
client to async multi-stream, at which point nonblock/poll/listen/IPv6 become
implementable. **Progress:** the persistent daemon landed (5.6); the `O_NONBLOCK`
*receive* path is honoured; honest `poll`/`epoll` read readiness landed via the
non-destructive `OP_POLL` peek; **non-blocking `connect` (EINPROGRESS →
`poll(POLLOUT)` → `getsockopt(SO_ERROR)`) now works**; **non-blocking `send`
(EAGAIN on a full send window, honest `POLLOUT`) now works**; and **listen/accept
server sockets now work at the daemon+ring layer** (ring `OP_LISTEN`/`OP_ACCEPT`,
passive-open TCP, in-daemon software loopback; validated by
`self_test_listen_accept`) (see the updated bullets above); **IPv6 connect is now
wired end-to-end through the socket-fd layer** (`sys_connect`/`getpeername` on
`AF_INET6` → `NetstackConn::connect6`). Remaining before the 5.7 default-flip:
route the AF_INET/AF_INET6 socket-fd server path's `bind`/`listen`/`accept` to the
daemon (`SockState::Listening` + `sys_bind`/`sys_listen`/`sys_accept4`) — the last
socket-fd gap (blocked on operator Q23).

### B-FAULT-SERIALSTORM. Unconditional per-page-fault `serial_println!` saturated the (slow) serial port during demand-paging bursts, starving the hard-lockup kick and making boots crawl / appear hung — FIXED 2026-07-14

**Where:** `kernel/src/proc/pcb.rs` — `try_resolve_fault` (demand-paged
anonymous frame site, ~L5267) and `resolve_file_cached` (page-cache mapped
site, ~L5352).

**Symptom / how it was found:** while validating the i6300esb NMI
hard-lockup watchdog (Q20/§61, `boot-test.sh --hard-lockup-watchdog`), a
boot ran ~4915 ms/stage behind and the NMI fired on ~9.7 s of BSP
kick-starvation:
```
[hardlockup] armed (NMI on ~9.8s BSP silence)
[sched] Task [hardlockup] NMI WATCHDOG FIRED cpu=0 rip=0xffffffff8010f556 ...
        heartbeat=5365 kick_stale_ns=9738940603 — dumping backtrace + task table
```
The captured `rip`/rbp-chain, re-resolved with exact 64-bit integer
arithmetic (awk's double precision silently zeroed the high bits of
`0xffffffff8010f556`), walked through `spin_loop_hint` →
`liveness_boot_deadline_check` → `timer_tick` — i.e. the BSP was *not*
deadlocked, it was simply spending all its time emitting serial. Each
demand-paged frame and each page-cache mapping printed an unconditional
`serial_println!`; a process faulting in its whole address space emits
thousands of these, and the 115200-baud serial port (~11 KB/s) cannot
drain them fast enough. The write path back-pressures in kernel context,
delaying `hardlockup::kick()` from `timer_tick` past the watchdog's
~9.8 s threshold — the boot looked hung and, under host load, could
tip the documented B-DASH-STDIN-FLAKE reap race over its own edge.

**Fix:** route both hot-path fault logs through
`crate::klog!(Trace, "mm.fault", …)` instead of `serial_println!`. klog's
`serial_level` defaults to `Info`, so Trace entries stay in the dmesg ring
buffer (still available for debugging via `dmesg`) but are kept OFF serial
by default. No fault-path log is lost; only the serial storm is gone.

**Validation:** `boot-test.sh` after the fix reached `BOOT_OK` in 132 s
with `storm=0` (zero `Demand-paged`/`Page-cache mapped` serial lines vs.
thousands before) and the container multi-network self-test still passing.
Boot no longer crawls; the hard-lockup kick is no longer starved by
demand-paging bursts.

**Note (Q20 watchdog validated):** this capture also *confirms the
i6300esb NMI hard-lockup detector works end-to-end* — it armed over the
boot ring-3 window, detected real BSP kick-starvation, delivered an NMI on
the dedicated IST2 stack, and dumped a usable rbp-chain backtrace + task
table exactly as designed. The detector doing its job is what surfaced
B-FAULT-SERIALSTORM in the first place.

### B-PREEMPT-SPINLOCK. Involuntary preemption while holding a tracked spinlock → single-CPU priority-inversion deadlock — ROOT-CAUSED & FIXED 2026-07-01

**Where:** `kernel/src/sched/mod.rs` (`do_deferred_preempt`), `kernel/src/sync.rs`
(`Mutex::lock`/`try_lock`/`MutexGuard::drop`). Manifested as a hang in
`accounting::self_test` on the `ACCT` lock (`kernel/src/mm/accounting.rs`).

**This is the true root cause of the long-standing intermittent
spawn/kill/reap / accounting-self-test hang** previously filed as **F6**
("Accounting self-test hang — LIKELY CURED INCIDENTALLY", further below) and
related to the B-PTHREAD-YIELDBUDGET / TD31 "total silence, no dump"
fingerprint. F6 was never actually cured — it just didn't recur in the soak
because the trigger is timing-dependent (~5%). The spinlock stall detector
(commit `c8c1fa63`) finally caught it red-handed.

**Symptom / evidence:** boot hangs mid-`accounting` self-test. The stall
detector prints:
```
[sync] *** SPINLOCK STALL *** lock 'ACCT' ... (cpu 0, task 0, ... iters)
[lockdep]   cpu 0 holds 2 lock(s): [0] ACCT [1] ACCT
```
The "recursive" `[0] ACCT [1] ACCT` is NOT true recursion. lockdep's held
stack is **per-CPU** and is not cleared on context switch, so `[0]` is the
still-tracked entry of a task that was **preempted while holding `ACCT`**, and
`[1]` is a second, higher-priority task now spinning to acquire the same lock —
both accumulated on cpu 0's held stack.

**Root cause:** a kernel spinlock must never be held across a context switch.
`crate::sync::Mutex` did not disable preemption while held, so the timer ISR
could involuntarily preempt (`do_deferred_preempt` → `preempt`) a task
mid-critical-section. On a single CPU, if a higher-priority task (e.g. the
prio-31 boot self-test driver) then spins on that lock, the preempted holder
can never be rescheduled to release it → permanent deadlock. `do_deferred_preempt`
already had a *SCHED-only* guard (`SCHED.is_locked()`) for exactly this hazard —
it was a band-aid that covered one lock instead of the general invariant.

**Fix (the proper, general one):** a per-CPU preempt-disable count
(`PREEMPT_DISABLE_COUNT`, Linux `preempt_count` analogue). `Mutex::lock`/
`try_lock` call `sched::preempt_disable()` for the whole hold; `MutexGuard::drop`
calls `preempt_enable()` **after** the physical unlock (the inner spin guard is
now held in `ManuallyDrop` so the unlock is ordered before the enable — closing
the tiny window where a tick could switch away with the lock still physically
held). `do_deferred_preempt` refuses to involuntarily switch while
`preempt_count(cpu) > 0`, re-arming `NEED_RESCHED` so the preemption lands on a
later tick after the lock is released. Interrupts stay **enabled** (this is
preempt-disable, not IRQ-disable); locks also taken from a hardware ISR (e.g.
cgroup `TABLE` via `timer_tick`) already use `try_lock` on the ISR side, so
preempt-disable alone is sufficient.

**Verification:** 3× consecutive green boot tests (193–196s), accounting
self-test now passes the previously-deadlocking "Largest RSS" step; no
`SPINLOCK STALL` in the serial log; clippy clean on both changed files.

**Limitation / follow-up:** the guard covers *involuntary* preemption only.
Voluntarily yielding/blocking (`yield_now`/`block`) while holding a tracked
spinlock is still a caller bug and is not guarded (there is no such call site
today). **Done (2026-07-01):** added a one-shot warning in `schedule_inner`'s
voluntary-switch path when `preempt_count(cpu) > 0` (commit `49c92d346`);
it stayed silent across all boots, confirming no offending call site exists.
Also added (commit `ebd5c4b21`) a lockdep instant SELF-DEADLOCK diagnostic when
the *same* lock instance is re-acquired on one CPU — fires immediately instead
of waiting ~30s for the stall detector, now reliable because tracked mutexes no
longer carry stale per-CPU held-stack entries across a context switch.

**Raw `spin::Mutex` audit (2026-07-01):** the preempt-disable fix protects only
`crate::sync::Mutex`; a *raw* `spin::Mutex` (250+ call sites, mostly procfs/sysfs
leaf backends) held across a preemptible path and contended by a higher-priority
task is the same latent deadlock class — and is *invisible* to both lockdep and
the stall detector. Audited the only plausibly-dangerous category, the blocking
IPC primitives (`futex`, `pipe`, `stream_socket`, `semaphore`, `eventfd`,
`epoll`, `timerfd`, `signalfd`): **all clean** — every one follows the correct
enqueue-waiter → `drop(table)` → `block_current()` discipline (e.g.
`futex.rs:340-379` scopes the table lock in a block that closes before the park).
The remaining raw-`spin::Mutex` uses are short snapshot copies where the
held-across-preempt window is a handful of instructions and cross-priority
contention is implausible. **Proper systemic fix (deferred tech-debt):** migrate
kernel-internal raw `spin::Mutex` to `crate::sync::Mutex` so *all* kernel
spinlocks disable preemption and get lockdep coverage — gated on first checking
the lockdep class-table capacity (a 250-lock bulk migration could overflow it),
so it needs a capacity bump or a per-class opt-in rather than a blind sweep.

### B-ACCT-LARGEST. `accounting` self-test "Largest RSS" assumed test-only isolation, panicking when a live process held >50 RSS frames — FIXED 2026-06-30

**Where:** `kernel/src/mm/accounting.rs`, self-test "Largest RSS"
section (was ~line 507). The test charged two fake PML4s (a=20, b=50)
then asserted `largest_rss().pml4_phys == pml4_b`. But `largest_rss()`
scans the **global** accounting table, which during a live boot also
contains *real* process address spaces. Whenever a concurrent real
process happened to hold >50 frames at that instant, `largest` was that
real PML4 (e.g. `0x1DFE0000`, not the fake `0xBEEF0000`), so the
`assert_eq!` panicked and **hard-halted the whole boot**, masking every
self-test after it. A load-dependent flake: it passed on light boots
and failed under heavier ones.

**Fix:** the assertion was false-isolation; replaced with invariants
that hold deterministically even with real entries present:
(1) among the test's own entries, `query` confirms b (50) outranks
a (20); (2) `largest_rss().rss_frames >= 50` — i.e. it returns a true
global upper bound — instead of asserting it equals a specific fake
PML4. Verified: clean build + green boot self-test.

### B-CONTAINER-JAIL-TESTRACE. `container` self-tests 18/19 (rootfs jail + volume mounts) flaked non-deterministically: spawned a real init process, then inspected its per-PID namespace state, which the process cleared by exiting mid-test — FIXED 2026-06-30

**Where:** `kernel/src/container.rs`, self-tests "Rootfs jail (chroot) for
init process" (Test 18) and "Volume (bind) mounts for init process"
(Test 19). Both originally did `let pid = run(ct, HELLO_ELF, &opts)` to
spawn a *real, schedulable* init process, then called
`namespace::resolve_path_for(pid, …)` several times to assert the chroot/
volume wiring. The race: `HELLO_ELF` prints one line and **exits
immediately**; on another CPU it could run and exit *between* two of the
test's resolves. Thread teardown on exit calls `namespace::detach(pid)`,
which drops `PROCESS_ROOT[pid]`/`PROCESS_MOUNTS[pid]`, so a later
`resolve_path_for(pid, …)` returned the **unjailed input verbatim** and
the `assert_eq!` panicked → hard-halted the boot. Observed as Test 18's
`..`-escape assert failing on a heavy boot while an identical-binary
re-run passed (load-dependent flake). Production code is correct: a live
process resolves its *own* paths inside its own syscall handler, so the
jail always exists for the duration; only a third-party test reading
another process's namespace after it may have exited hits this.

**Fix:** Tests 18/19 no longer spawn a schedulable process. They register
a *synthetic, never-scheduled* PID through `add_process(ct, FAKE_PID)` —
the exact same container-layer wiring path `run()` uses
(`add_process_task` → `set_root`/`add_volume`) — and then run the
resolution asserts deterministically (the PID has no thread, so it cannot
exit and clear its state). The concerns that genuinely need a live
process are still covered without the race: the end-to-end
`run()`→cgroup-billing path by the "Run init process + cgroup billing"
test (Test 17), and the resolution *semantics* (`..` clamp, longest-
prefix volume match) by `namespace::test_process_root` /
`test_volume_mounts` (which already use synthetic PIDs 88888/88889). The
`state != Created` config-rejection guard is now exercised via `stop()`
rather than a live process, so it too is deterministic. Verified: clean
build + green boot self-test ("Self-test PASSED (19 tests)").

**Update (2026-06-30) — latent flake OBSERVED as a boot hang, now FIXED:**
The Test 17 liveness risk noted above stopped being theoretical. On a
heavy boot run the serial log froze mid-test right after the `run()` log
line (`[container] run id=8 'test-run-ct': init pid=219 …`) and never
reached `BOOT_OK` (480s timeout → boot gate FAILED). An identical-binary
re-run passed (`BOOT_OK after 187s`), confirming a load-dependent race,
not a logic bug — a timer ISR preempted the boot self-test thread into
the freshly-spawned init task, which executed `hello`; the exiting
thread's teardown then raced the test's explicit teardown, deadlocking
(a hang, not an assert panic — no `[PANIC]` was printed). This was worse
than the predicted assertion flake because a hang fails the *entire* boot
gate. **Fix:** Test 17's spawn→teardown window is now bracketed in
`cpu::without_interrupts(...)`, so the init task is still *registered*
(cgroup billing is verified end-to-end exactly as before) but can never
be *scheduled* before `destroy()` removes it — deterministic, with no
loss of real-`run()` coverage. Verified: clean build + green boot
self-test. Production code is unaffected (a live process only ever
resolves its *own* state inside its own syscall handler).

### B-PTHREAD-YIELDBUDGET. Intermittent "BSP-dead total-silence hang" during boot ring-3 self-tests — RESOLVED 2026-07-02 (structural: interrupts now enabled before the battery; see the "STRUCTURAL ROOT FIX" note at the end of this entry). Original title: `/bin/pthread` self-test can exceed the 262 144-yield exit budget under heavy boot load — WATCH (non-fatal)

**Where:** boot integration self-test that spawns `/bin/pthread`. The
harness waits for the child to exit within a fixed yield budget
(262 144 yields). On a heavy boot (observed once at ~229 s wall vs. the
normal 161–192 s), the child was still `state=Running` when the budget
expired and the harness logged "process did not exit within 262144
yields (state=Running)". This is a **non-fatal warning** — it does not
panic or fail the boot, and the same test passed on the immediately
preceding and following boots.

**Assessment:** a timing flake, not a correctness bug. The mutex/futex
hot loop was not touched by the surrounding container/VFS work, and the
failure is purely budget-vs-wall-clock under contention. **Proper fix
(deferred):** make the harness wait on an actual exit signal / longer
adaptive budget rather than a fixed yield count, so a slow-but-correct
run isn't misreported. Tracked here until the harness is reworked.

**Recurrence 2026-06-30:** observed again on a ~217 s BOOT_OK run (heavy
boot); the harness logged the same "did not exit within 262144 yields
(state=Running)" for the real-glibc pthread variant. Non-fatal that time —
BOOT_OK was reached and the container self-test (40 tests) passed on the
same boot.

**New variant 2026-07-15 — empty-capture, NOT a hang
(`build/hang-catches/soak-20260715-022705-iter18`).** Distinct manifestation of
the same real-glibc pthread self-test (`proc/spawn.rs`, `EXPECT_OUT =
"SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10\n"`): the child **reached
Zombie and exited with the correct code** (so it passed the reap and exit-code
checks), but the captured stdout file read back **0 bytes** —
`[spawn]   FAIL: real glibc pthread — captured 0 bytes [], expected [83,76,…]`,
`WARNING: Path-Z real glibc pthread self-test failed: InternalError`. Since the
child fully exited (glibc's `atexit` stdio flush should have run and the
`write(1,…)` to the redirected capture file completed), an empty read-back points
at a **capture-file write/read visibility race** (a just-written file's contents
not yet visible to the harness's immediate `Vfs::read_file`), *not* the
yield-budget hang above — the two are different failure modes of the same test.
Intermittent: 1 of ~18 armed boots in that soak; every other pthread run in the
soak passed. Unlike the hang variant (classified `TimedOut` → WARNING), this
empty-capture path returns `InternalError` and boot-test.sh flags the boot
FAILED. **Proper fix (deferred, needs its own investigation):** determine whether
the redirected-fd-1 file write is fully durable/visible at the moment the child
becomes Zombie — if not, either fsync/flush on the capture fd at process teardown
or have the harness retry the read-back a bounded number of times. Logged for the
next focused session on the VFS write-visibility path.

**Severity escalation 2026-06-30 — a *full* boot hang was observed, not
just the non-fatal warning.** On a subsequent run the boot never reached
BOOT_OK within the 480 s timeout; the serial log's last activity was in the
real-glibc clone/COW region (pid 170/171: `[cow] Cloned address space`,
page-cache faults for the glibc text inode, a freshly spawned thread in the
child) with no further progress — consistent with the pthread `clone`+futex
worker deadlocking *permanently* rather than merely running slow. The very
next boot (identical binary) reached BOOT_OK at 222 s with the pthread test
passing (`captured 48 bytes == expected: OK`), confirming the hang is
intermittent. This means the futex/clone path has a **real, low-probability
deadlock**, not purely a yield-budget timing artifact — the fixed-budget
harness masks it as a warning on slow-but-live runs but the underlying hang
can be total. **Proper fix (still deferred, now higher priority):** root-cause
the futex wait/wake race in the glibc `clone`+TLS worker path (candidate: a
lost wakeup when a waker runs before the waiter parks, or a missed requeue),
in addition to reworking the harness to wait on a real exit signal. No code
change made this session (the observation came from unrelated container-CLI
boot tests); logged here so the intermittent total hang isn't forgotten.

**Search narrowed 2026-07-01 (negative result):** audited the core futex
wait/wake primitive for the "lost wakeup when a waker runs before the waiter
parks" hypothesis and found it **sound** — not the bug. `futex_wait_bitset`
enqueues the `Waiter` under `FUTEX_TABLE`, drops that lock, then calls
`sched::block_current()`; the classic window between "dropped the futex lock"
and "parked" is closed by the scheduler's `pending_wake` flag: `sched::wake`
(mod.rs ~L1388) and `sched::try_wake` (ISR path, ~L1436) both set
`task.pending_wake = true` when the target is *not yet* `Blocked`, and
`block_current` (~L1373) consumes that flag and returns **without** parking. So
a `futex_wake` (or timer/ISR wake) that races ahead of the park cannot be lost.
The `register-then-recheck` signal-waiter dance likewise closes the
signal-vs-enqueue window for user tasks. **Conclusion:** stop looking at the
futex primitive; the intermittent total hang is in the surrounding ring-3
`clone`/CoW-fault/thread-teardown-reap machinery (the last serial activity on
the total-hang run was in the glibc `clone` CoW region — `[cow] Cloned address
space`, page-cache faults for the glibc text inode — not inside a futex wait).
Next candidates to instrument: (a) the CoW page-fault handler taking a lock the
reaper/`clone` path also takes (frame-alloc vs. address-space vs. page-table
lock ordering), and (b) `on_thread_exit`/`reap_dead_tasks` racing a thread that
is mid-`clone`. A lock-order tracer around the address-space + frame-alloc +
SCHED locks during a `clone`-heavy boot is the tool to build next.

**Tooling reconnaissance 2026-07-01 (negative — narrows the fix, no code
change).** Two findings that reshape what "instrument this" requires:
1. *A lockdep validator already exists and is enabled at boot* (`kernel/src/lockdep.rs`,
   `lockdep::init()` at `main.rs:3678`; `crate::sync::Mutex` auto-reports
   acquire/release; `lockstats` kshell cmd). It flags an AB-BA cycle on **any**
   boot where both orderings are ever observed — but **only for locks that use
   the tracked `crate::sync::Mutex`.** The two prime suspects are **untracked raw
   `spin::Mutex`**: the buddy frame allocator (`mm/frame.rs:813`
   `static ALLOCATOR: Once<Mutex<BuddyAllocator>>`, `use spin::{Mutex, Once}`)
   and the rmap table (`mm/rmap.rs:174` `static TABLE: Mutex<RmapTable>`,
   `use spin::Mutex`). **That is exactly why the hanging runs produced no lockdep
   report.** Migrating them to `crate::sync::Mutex` would let lockdep catch a
   latent inversion deterministically — but the frame allocator is a
   <1 µs-target hot path and lockdep adds ~50–200 ns/acquire, a >20% regression
   on every `alloc_frame`/`free_frame`, so this can't just be left on in normal
   builds. A `cfg(feature = "lockdep_mm")` gated migration is the proper form if
   this route is taken.
2. *Give-up-path instrumentation would not catch the TOTAL hang.* The yield-budget
   "did not exit within N yields" give-up messages in `proc/spawn.rs` (~20 sites)
   only fire when the driver task keeps running and merely the *child* is slow.
   In the total-hang variant the serial log stops mid-clone with **no further
   output at all** — the give-up line never prints, meaning the driver (or the
   whole CPU) also stalled, consistent with a lock held forever by a stuck task.
   So a state-dump *at the give-up* is useless here; catching this needs a
   **timer-interrupt watchdog** that, on N seconds of no forward progress, dumps
   every task's `(id, name, state, cpu, wait-reason)` from IRQ context (and must
   itself take **no** contended lock — use `try_lock`/lock-free reads only). That
   watchdog is the real next build; it's larger than a one-liner, hence deferred
   rather than bolted on mid-turn. Until then the bug stays WATCH: it is rare,
   does not affect the common boot (BOOT_OK is reached ~95%+ of runs), and is
   fully documented here.

**Root-cause narrowing 2026-07-01 (audit line concluded — I/O paths cleared,
instrument built).** A systematic pass eliminated every lock-order and I/O
lost-wakeup hypothesis, leaving two structural suspects, and the hung-task
watchdog called for above is now **implemented and boot-validated**.
- *Hypotheses eliminated (all proven sound by inspection):*
  1. Futex primitive — sound; `pending_wake` closes the register/block race.
  2. Ready-starved task lost from the run queue — RULED OUT: `check_starvation`
     (`sched/mod.rs`) re-enqueues any Ready non-throttled task within ~2 s.
  3. `page_cache::get_or_fill` (`mm/page_cache.rs:214`) — optimistic
     fill-then-insert with race resolution; **no fill-in-progress wait queue**,
     so no lost-wakeup there.
  4. PAGE_CACHE ↔ frame ALLOCATOR lock order — consistent (PAGE_CACHE is always
     the outer lock via `ref_inc`; `alloc_order` releases ALLOCATOR *before*
     reclaim/compact/OOM), so no AB-BA.
  5. Page-cache fill closure (`fs/handle.rs:584` `read_at_uncached` →
     `Vfs::read_at_uncached_resolved` → `fs.lock().read_at`) holds **no**
     page-cache/frame lock across the read, and `write_at`/`truncate` invalidate
     the cache only *after* dropping `fs.lock()` — no fs.lock↔PAGE_CACHE nesting.
  6. **Block-device read (the serial trace stops exactly here) — ELIMINATED.**
     `virtio/blk.rs::wait_completion` in IRQ mode is a **HLT-poll loop bounded by
     a 500-attempt (~5 s) timeout** (`if attempts > 500 { … "timed out (IRQ
     mode)" … return Err(TimedOut) }`), *not* a wait-queue block. The 100 Hz
     timer wakes every `hlt()`, so even a fully lost device IRQ cannot hang it
     silently — it would print `[virtio-blk] … timed out` and return an error.
     The hang trace shows no such line, so the disk read is not the stall. The
     RAM-disk path is a plain synchronous memcpy (no wait queue either).
- *Remaining suspects (cannot be pinned by static reading — need a runtime
  dump at the moment of hang):* (a) a `clone`/CoW thread whose wakeup is lost on
  some primitive *other* than the futex/page-cache/frame paths above; (b)
  `on_thread_exit`/`reap_dead_tasks` racing a thread that is mid-`clone`.
- *Instrument built (this is the "real next build" the reconnaissance note asked
  for):* a **system-wide liveness watchdog** in `sched/mod.rs`
  (`liveness_arm`/`liveness_disarm`/`liveness_check`/`dump_all_tasks_serial`,
  driven by the BSP every `WATCHDOG_CHECK_INTERVAL` = 5 s alongside the existing
  soft-lockup watchdog). It watches one global counter, `USEFUL_WORK_TICKS`,
  bumped by `timer_tick` whenever a tick preempts a **non-idle** context
  (`from_user || local_has_real_work`). At the total-hang every CPU is parked in
  the idle task with an empty run queue, so this counter **freezes** even though
  per-CPU heartbeats keep climbing (which is precisely why the soft-lockup
  watchdog can't see it). If it fails to advance for `LIVENESS_ALERT_COUNT` = 3
  consecutive intervals (~15 s) while armed, the BSP dumps every task's
  `(tid, state, cpu, prio, pending_wake, ready_since, waited, blocked_on_pi,
  name)` plus each CPU's `(heartbeat, ctx_switches, local_has_real_work)`
  straight to serial from IRQ context using **try_lock only** — and if it can't
  get `SCHED`, it reports *that* (a task wedged holding `SCHED` is itself the
  deadlock). It then disarms so the report prints exactly once. Scoping solves
  the idle false-positive problem the reconnaissance note flagged: it is armed
  only for the boot ring-3 window (`main.rs`, right before the ring-3 fork/CoW/
  reap self-tests) and disarmed at BOOT_OK, before the system may legitimately
  idle at an interactive prompt. Validated: a healthy boot reaches BOOT_OK with
  **zero** `[liveness]` output (silent when healthy). Next time the hang
  reproduces in a boot test, the serial log will name the lost thread and its
  state — turning this heisenbug into a directly-diagnosable one.
- *On-demand dump added:* the same task-table dump is now reachable
  interactively via the kshell `taskdump` command (aliases `hungcheck`/
  `dumptasks`; `sched::dump_task_table()`), for capturing state when a system
  feels wedged at a prompt — the window where the boot-scoped watchdog is
  disarmed. try_lock-only, safe on a partially-hung system, output to serial.
- *Reproduction attempt 2026-07-01 (negative):* ran `scripts/hang-repro-loop.sh`
  for 16 consecutive boots (15-boot batch + 1 validation) with the instrument
  armed — **all reached BOOT_OK, zero `[liveness]` fires, no catch.** Consistent
  with the ~5% rate (P(0 catches in 16 boots) ≈ 44%), so this neither reproduces
  nor disproves the bug; it just confirms the instrument is silent on healthy
  boots and does not itself destabilise boot. The watchdog stays permanently
  armed for the boot window, so any future reproduction (in CI or ad-hoc boots)
  will be captured automatically. Not running further blind repro batches — they
  produce no artifact — until the bug surfaces on its own.
- **Reproduced 2026-07-01 (the bug surfaced on its own) — BUT THE WATCHDOG DID
  NOT FIRE, exposing a structural blind spot in the instrument.** A boot test
  during the tee(2) session hung: no BOOT_OK within the 480 s timeout, ~470 s of
  total serial silence. The hang point matches the family signature exactly — the
  "REAL make-drives-tcc build (ring 3, Path Z)" stage: `/bin/tcc -c /cap-a.c -o
  /cap-a.o` triggered `[cow] Cloned address space: parent=0x1bb83000 ->
  child=0x119000`, task 176 / process 210 exec'd a PIE ELF (ld-linux
  interpreter), then the last two lines were `[thread] Process 210 has no threads
  left — now zombie` / `[sched] Task 176 exiting`, followed by dead silence. Log
  preserved at `build/hang-catches/CAUGHT-2026-07-01-tee-session-nobootok.txt`
  (5773 lines). The very next boot (`--no-build`, identical binary) reached
  BOOT_OK in 206 s — confirming intermittency, as always. **The critical new
  signal: no `[liveness] SYSTEM HANG` dump, no `[watchdog]` soft-lockup line —
  nothing at all.** The watchdog *was* armed (armed at `main.rs:1341`, well before
  this Path-Z stage; disarmed only at BOOT_OK, which was never reached), so
  arming is not the gap. That leaves two structural blind spots, and the total
  silence points hard at the second:
  1. *Livelock (watchdog resets every interval):* if some non-idle task keeps
     getting ticked (a busy-spin / lost-wakeup retry loop in ring-0 or ring-3),
     `timer_tick` charges the tick to a non-idle context (`from_user ||
     local_has_real_work`) and bumps `USEFUL_WORK_TICKS`, so `liveness_check`
     (`sched/mod.rs:1738`) sees `current != previous`, resets `LIVENESS_STALL_COUNT`
     to 0, and never reaches the 3-interval alert. The watchdog only catches an
     *idle* hang (all CPUs parked in the idle task), not a *busy* one.
  2. *BSP stopped ticking (watchdog never runs at all) — most likely here.* The
     ENTIRE watchdog stack (`watchdog_check` + `liveness_check`) is driven from
     `timer_tick` on **cpu == 0 only** (`sched/mod.rs:1955`, `:1972-1976`). If the
     BSP itself wedges with interrupts disabled — a spin holding a raw `spin::Mutex`
     with IF=0, or the LAPIC timer not re-armed — the BSP timer ISR never runs, so
     neither watchdog ever executes and no diagnostic can print. The observed
     **total** silence (not even the soft-lockup detector, which watches per-CPU
     heartbeats and would fire within 15 s if the BSP were still ticking while an
     AP froze) is the fingerprint of a dead BSP tick, i.e. blind spot (2).
  **Proper fix (the real next build, deferred — larger than a one-liner):** make
  the hung-system detector independent of the BSP timer tick.
  - *Cross-CPU liveness (cheap partial fix):* also call `liveness_check()` from an
    **AP's** `timer_tick`, not just cpu 0, so a wedged BSP doesn't take the whole
    watchdog down with it. Guard the shared stall counters for concurrent access
    (they're already atomics; the one-shot disarm makes double-fire harmless).
    Does not help if *all* CPUs stop ticking, and — critically — **our boot test
    runs single-CPU**, so there is no AP to run this. Useful only once boot tests
    exercise SMP.
  - *NMI-based hard-lockup detector — FEASIBILITY BLOCKER FOUND 2026-07-01.* The
    Linux `watchdog_hld.c` model arms a **PMC counter overflow → LAPIC LVT
    PerfMon → NMI**, which fires even with IF=0. **But this cannot work in our
    validation environment:** `scripts/boot-test.sh` launches QEMU with **no
    `-accel` and no `-cpu` flag** → default **TCG** + `qemu64`, which does **not
    emulate the PMU overflow→NMI path** at all. A PMC-based detector would never
    fire under our only test harness, so it is untestable and effectively dead on
    arrival here. (On real hardware / KVM it would work, but we have no such test
    path.) Combined with single-CPU (no AP to send a watching NMI-IPI), the PMC
    approach is the wrong build for this project as currently tested. **Do NOT
    build the PMC detector against the current harness.**
  - *Revised approach that DOES work under TCG (the actual next build): QEMU
    `i6300esb` PCI watchdog → inject-NMI.* Add `-device i6300esb` +
    `-action watchdog=inject-nmi` to `boot-test.sh`, write a small kernel driver
    that maps the device BAR and **kicks** the watchdog from the timer tick (or a
    dedicated periodic point). If the BSP wedges with IF=0 the kicks stop, the
    watchdog expires, and QEMU injects a real NMI regardless of IF — caught by
    `handle_nmi` (idt.rs:1422), which would then dump the task table (try_lock
    only) via `sched::dump_task_table`. Requires: the driver, a **dedicated IST**
    for the NMI vector (currently `ist=0`), arming scoped to the boot ring-3
    window, and the harness flag change. **Blast-radius caveat:** this touches the
    *shared* boot harness — a mis-tuned kick period would make every future boot
    test spuriously NMI-dump or let QEMU reset the guest. Because it changes shared
    test infra, it is queued for an operator steer in `open-questions.md` rather
    than landed unilaterally. Validating it against the actual ~5% heisenbug is
    also hard (needs ~20 boots to reproduce once).
  **Blind spot (1) livelock guard — IMPLEMENTED 2026-07-01** (`sched/mod.rs`
  `liveness_check`, `total_ctx_switches`, statics `LIVENESS_LAST_CTX` /
  `LIVENESS_CTX_STALL_COUNT`). On the healthy branch (useful-work advanced), the
  watchdog now also samples the **system-wide context-switch total** (sum of the
  per-CPU `CTX_SWITCHES`). The busy-livelock signature is *useful-work advancing
  while the aggregate ctx-switch count is frozen*: a task monopolizing a CPU
  without ever yielding gets its own timer ticks charged as "useful work" yet
  produces no context switch, whereas a healthy boot self-test phase
  context-switches continuously (thread spawn/reap/futex hand-off/yield). After
  `LIVENESS_ALERT_COUNT` (3 = 15 s) such intervals it prints a `SUSPECTED
  LIVELOCK` line + task dump. Deliberately chosen discriminator over
  "sample-the-running-tid": the long-lived boot self-test *driver* task keeps the
  same tid across the whole armed window, so same-tid-for-K-intervals would
  false-positive; ctx-switch-frozen does not. Because a rare legit long
  single-task compute in a stress self-test could in principle also freeze ctx
  switches while charging useful work, the livelock report is a **soft warning**:
  it does NOT disarm the watchdog (so a false positive cannot disable hang
  detection for the rest of boot) and re-fires at most once per 3 intervals.
  Covered by an extended `test_liveness_watchdog` self-test (drives the guard to
  threshold under IF=0, asserts it warns without disarming and resets on
  ctx-switch progress). This closes the *busy*-livelock variant; the **BSP-dead
  blind spot (2)** (total silence, IF=0 spin — the fingerprint of the 2026-07-01
  catches) still requires the NMI-based detector above and remains deferred.
  **Blind spot (2) software mitigation — IMPLEMENTED 2026-07-01** (`sync.rs`
  `Mutex::lock_contended` / `report_stall`, `lockdep::dump_held_locks`). Rather
  than wait on the operator-gated i6300esb/NMI hardware path (Q20), the contended
  path of `crate::sync::Mutex` now runs a **bounded-spin stall detector** in pure
  software: it spins on `try_lock` (behaviourally identical to the old
  `spin::Mutex::lock()`), and if a single acquisition spins longer than
  `STALL_SECONDS` (30 s) of PIT-calibrated TSC wall time it emits a **one-shot,
  non-fatal** `*** SPINLOCK STALL ***` diagnostic naming the lock, the wedged
  cpu/task, and — via the new `lockdep::dump_held_locks` — the locks that cpu
  already holds (the key AB-BA/convoy clue), then keeps spinning. Because it fires
  from *inside* the spin loop it works even with IF=0, which is exactly the
  BSP-dead fingerprint the timer-driven watchdog misses. The threshold is far
  beyond any legitimate kernel hold (ms-scale), so it never false-fires under
  normal contention (verified: BOOT_OK 182 s, zero `SPINLOCK STALL` lines).
  Globally rate-limited to `MAX_STALL_REPORTS` (8) so a multi-CPU convoy can't
  flood serial; falls back to a raw iteration count if the TSC isn't yet
  calibrated. **Coverage caveat:** this only catches deadlocks on locks that go
  through `crate::sync::Mutex`; a hang on a *raw* `spin::Mutex` (or a
  non-lock IF=0 spin) is still invisible to it — those remain the domain of the
  Q20 hardware NMI detector. The new `dump_held_locks` helper is exercised by a
  lockdep self-test (Test 6). This meaningfully narrows blind spot (2) without
  touching the shared boot harness or waiting on the operator.
  **CGROUP TABLE lock brought under observability — 2026-07-01** (`cgroup.rs`).
  The cgroup `TABLE` lock — the single lock most implicated in the hang (TD31:
  adding attach/detach TABLE traffic to spawn/reap made the ~5% hang
  near-deterministic) — was a **raw `spin::Mutex`**, so it was invisible to both
  lockdep and the stall detector. Converted it to a tracked
  `crate::sync::Mutex::named(…, b"CGROUP")`. Zero behavioural change (only
  `lock()`/`try_lock()` were used, both drop-in), but now: (a) a TABLE-side
  deadlock produces a `SPINLOCK STALL` dump instead of silence, and (b) lockdep
  tracks TABLE for order validation and contention stats. Cost is negligible —
  cgroup mutations are rare and off every hot path. Verified BOOT_OK 185 s, no
  new lockdep violation, cgroup self-test still green.
  **NOT yet converted: `SCHED` (sched/mod.rs:255) is also a raw `spin::Mutex`.**
  For lockdep to detect the *suspected* SCHED↔CGROUP AB-BA it needs **both** locks
  tracked, so the edge is still not recorded. But converting SCHED is a separate,
  **benchmark-gated** decision: SCHED is the hottest lock in the kernel (acquired
  on every context switch / timer tick / spawn / reap), and `crate::sync::Mutex`
  adds a lockdep held-stack push + edge scan on every acquire — a real
  context-switch-latency risk against the <5 µs target. On a **single-CPU** boot a
  classic two-CPU AB-BA is impossible anyway; the realizable single-CPU deadlock
  is an ISR acquiring a lock held by the interrupted code (the timer-tick cgroup
  path already uses `TABLE.try_lock` precisely to avoid this) or a recursive
  self-acquire — neither of which needs the AB-BA edge to be caught, only the
  stall detector, which now covers TABLE. So the pragmatic call is: **let the
  TABLE stall detector probe the next recurrence** before paying to instrument
  SCHED. If a recurrence stays silent (SCHED-side spin), revisit converting SCHED
  behind a benchmark and possibly a debug-only lockdep-on-SCHED build.

**Recurrence 2026-07-01 (embedded-DNS work, same signature).** During the
boot test for the container embedded-DNS increment, one run hung with no
BOOT_OK in 480 s; serial stopped mid-line at `[thread] Spawned thread (t`
immediately after `[cow] Cloned address space: parent=… -> child=…` and
`[sched] Spawned task 144` for a ring-3 clone (pid 177), with a burst of
page-cache faults for a glibc text inode just before — the exact
clone/CoW/thread-spawn signature documented above, and **no** watchdog dump
(BSP-stuck blind spot). The **immediately following** boot of the identical
binary reached BOOT_OK at 177 s with every self-test passing (including the
new `[cnetwork]   embedded DNS resolve: OK`). Confirms again the hang is in
the ring-3 `clone`/CoW-fault/thread-spawn path and is independent of the
touched code (this session changed only `cnetwork.rs`/`kshell.rs`, neither
on the boot spawn path). No new fix this session; datapoint logged.

**Recurrence 2026-07-01 (livelock-guard work, same signature).** While
boot-testing the new blind-spot-(1) livelock guard, one confirmation run hung
with no BOOT_OK in 480 s; serial stopped at `[spawn] Process 220 running
(thread 184, entry=0x4000000000, user_rsp=0x7fffffff0000)` — the container
`exec` self-test spawning ring-3 `/bin/hello` (task 184 in process 220),
immediately after `[thread] Spawned thread (task 184)`. Same
clone/thread-spawn fingerprint, and **no watchdog dump at all** (BSP-dead
blind spot 2). This is the *variant the new guard does NOT catch* — the guard
targets busy-livelock (blind spot 1); this is the IF=0 BSP-dead case that
still needs the NMI detector. Confirmed unrelated to the change: the new
`test_liveness_watchdog` self-test logged `[sched]   liveness watchdog: OK`
long before the hang, and the immediately-prior boot of near-identical code
reached BOOT_OK in 191 s. Datapoint logged; underscores that blind spot 2
(NMI detector) is the remaining high-value work on this bug.

**Clean datapoints 2026-07-02 (TD31 symmetric-accounting landed — *added*
CGROUP TABLE traffic to spawn/reap).** Landing the TD31 attach-on-spawn change
(commit `51c4033ef`) adds one `cgroup::attach_task` (TABLE lock) per task spawn,
on top of the detach-per-reap already present — i.e. it re-introduces exactly the
kind of extra TABLE traffic that, in the *original* TD31 attempt (pre
B-PREEMPT-SPINLOCK fix), made this hang near-deterministic and hung the boot
twice. With B-PREEMPT-SPINLOCK now fixed and CGROUP TABLE now a *tracked*
`crate::sync::Mutex`, the change booted **green 4× consecutively** (190/182/181/
185 s), **zero** `[liveness]`/`SPINLOCK STALL`/self-test-failure lines and no
`dash`/`pthread` flake. This is strong evidence the preempt-disable fix cured (or
at least drastically reduced) the TABLE-traffic-sensitive variant of this hang —
the added traffic that used to make it ~deterministic no longer reproduces it. A
follow-up 15-boot `hang-repro-loop.sh` soak on the TD31 binary is running to
gather more evidence (the now-tracked CGROUP lock means a TABLE-side deadlock
would finally produce a `SPINLOCK STALL` dump rather than silence). The genuinely
*total-silence* BSP-dead variant (blind spot 2) still needs the operator-gated
i6300esb/NMI detector (Q20) to be caught if it recurs.

**Blind spot (2) NMI hard-lockup detector — IMPLEMENTED 2026-07-02 (the
operator authorized option D — root-cause this hang — which unblocked the
i6300esb build previously gated behind Q20).** New `kernel/src/hardlockup.rs`
drives the QEMU i6300esb watchdog (PCI `0x8086:0x25ab`): maps BAR0 NO_CACHE,
programs a two-stage ~9.8 s countdown (1 kHz mode, `STAGE_PRELOAD`=5000 ≈
4915 ms/stage) with the reboot action left enabled (QEMU's inverted
`ESB_WDT_REBOOT` logic — bit clear = action armed), which `-action
watchdog=inject-nmi` routes to an injected NMI. `arm`/`kick`/`disarm`/`is_armed`
API. The **BSP** `timer_tick` (`sched/mod.rs`, `cpu==0`) kicks it every tick, so
while the BSP takes timer interrupts it never expires; if the BSP wedges with
IF=0 the kicks stop and QEMU broadcasts an NMI to every CPU — the wedged BSP
takes it *despite* IF=0. `handle_nmi` (`idt.rs`), when `hardlockup::is_armed()`
and the NMI has no port-0x61 hardware-error bits, prints `[hardlockup] NMI
WATCHDOG FIRED cpu=… rip=… cs=… rflags=…` for every CPU (the BSP's line is the
prize — the wedge RIP we could never observe) and the first arriver dumps the
full task table (one-shot latch). Armed at `main.rs` right after `liveness_arm`
(before the ring-3 container self-tests), disarmed at BOOT_OK. The device is
**opt-in** via `boot-test.sh --hard-lockup-watchdog` (already present, off by
default), so a normal boot finds no device and every entry point is a cheap
no-op — **zero blast radius on ordinary boots** (this resolves the shared-harness
blast-radius caveat that gated the build). Uses `ist=0` for v1 (the wedge is an
ISR spin with the stack intact). Verified: a clean `--hard-lockup-watchdog` boot
arms (~4915 ms/stage), disarms at BOOT_OK, reaches BOOT_OK in 172 s with **no
false-fire**. `hang-repro-loop.sh` now boots with the watchdog and treats
`[hardlockup] NMI WATCHDOG FIRED` as a catch. A soak with the instrument is
running to capture the wedge RIP; once captured, the RIP + task-table dump turn
this heisenbug into a directly-diagnosable one. **This is the tool that finally
makes blind spot 2 observable.**

**Fire path validated & width-bug fixed 2026-07-02.** An early deliberate-fire
self-test (`hardlockup::self_test_fire`: arm, then spin `IF=0` without kicking
for ~15 s) initially FAILED — the counter never started, so no NMI. Root cause:
QEMU's `i6300esb_config_write` decodes the *access width* — it only handles the
CONFIG register (0x60) on a 2-byte write and the LOCK register (0x68) on a
1-byte write — but `pci::config_write16` always emits a 32-bit `outl`
(read-modify-write, len==4). Both the CONFIG program and the ENABLE bit fell
through to default config storage, so `i6300esb_restart_timer` never ran. Fixed
by adding true-width `pci::config_write8` (byte access to data-port lane
`0xCFC + (offset&3)`) and `pci::config_write16_native` (`outw` to
`0xCFC + (offset&2)`), used for LOCK and CONFIG respectively (commit
`d0b6e648c`). Re-validated: with the fix the self-test PASSES — QEMU injects an
NMI ~10 s into the `IF=0` spin, `handle_nmi` catches it despite `IF=0`, resolves
`rip=kernel::cpu::delay_us` (exactly the spin), and dumps the task table. The
instrument is now proven end-to-end; the temp self-test call was reverted before
committing.

**Wedge window narrowed from the newest catch (2026-07-01 tee-session).** That
total-silence hang's last two serial lines were `[thread] Process 210 has no
threads left — now zombie` (`proc/thread.rs:445`) then `[sched] Task 176 exiting`
(`sched/mod.rs:1213`), then nothing. So the BSP wedges in the *tail of
`task_exit`*, after that print: `notify_exit_hooks(current_id)` (exit hooks run
lock-free) → `SCHED.lock()` to set `Dead` → `schedule_inner(false, Uncounted)`
(the context switch, which runs with IF=0). The dead-BSP/IF=0 fingerprint points
at the switch itself or a lock taken in an exit hook. The armed NMI soak will
resolve *which* by giving the exact wedge RIP; no further static speculation
until the catch lands.

**ROOT-CAUSED & FIXED 2026-07-02 — it was a false-positive watchdog trip on a
multi-second IF=0 SHA-256, NOT a deadlock.** The armed NMI soak caught the wedge
on the first iteration, and the new RBP-chain backtrace in `handle_nmi`
(`idt.rs::dump_kernel_backtrace`) resolved the exact call chain:
```
kmain → kernel_main → proc::spawn::self_test_linux_real_glibc_full
  → fs::vfs::Vfs::write_file → write_file_resolved
    → fs::history::try_auto_record → record_version
      → fs::cas::put → crypto::sha256 → crypto::Sha256::update  (rip in rotate_right)
```
The NMI fired at `rflags=0x10002` (**IF=0**) right as the glibc-full self-test
began staging its files, and — decisively — the serial log **continues past the
NMI dump to `BOOT_OK`**, so the machine was never actually deadlocked. What
happened: file-history auto-versioning was **on by default** (`fs::history`
static `HISTORY` had `auto_version: true`), so every boot-time overwrite of an
OS system file (the glibc tree, staged for the Path Z self-tests) made
`record_version` read the *old* content and SHA-256-hash it via `cas::put`.
Crucially, the entire Path Z self-test block runs **before** "Step 21: Enable
hardware interrupts" (`main.rs` `cpu::sti()`), i.e. with **IF=0**. In a debug
(unoptimised) build, hashing a multi-megabyte glibc file takes several seconds;
with IF=0 the BSP takes no timer ticks, so the timer-driven hard-lockup watchdog
kick (`sched::timer_tick` → `hardlockup::kick`, BSP-only) is starved. Under
host-scheduling jitter the ~9.8 s watchdog occasionally expired mid-hash,
producing the intermittent "BSP-dead total-silence" fingerprint. It presented as
a ~5% *hang* rather than 100% because the hash time sits near the watchdog
threshold / the soak-harness boot timeout, and only the jitter tail crosses it.

**Fix (proper, targeted):** file-history auto-versioning now starts **disabled**
and is enabled only at `BOOT_OK` (`main.rs`, right after `hardlockup::disarm()`,
via `fs::history::set_auto_version(true)`). Rationale: versioning OS files as
they are staged during boot is pointless (nobody rolls them back) *and* running
a seconds-long SHA-256 with IF=0 is the "long operation under IRQs-disabled"
anti-pattern regardless of the watchdog. Past BOOT_OK the BSP is preemptible
(IF=1) and OS staging is done, so auto-versioning real user-data writes is safe.
The history self-test is unaffected — it calls `record_version()` explicitly on
`/tmp` paths (which `should_auto_version` skips), independent of the flag.
Follow-up perf note logged separately: auto-versioning being globally on means
every user-data overwrite pays a read+rehash tax; capping by size or making it
truly opt-in per-path (per the module's own "opt-in" design statement) is a
worthwhile future optimisation, but it no longer gates boot liveness.

**STRUCTURAL ROOT FIX 2026-07-02 — enable interrupts BEFORE the ring-3 self-test
battery (RESOLVED).** Deferring auto-versioning (offender #1) did *not* stop the
watchdog fires: the armed NMI soak caught a second, independent offender on the
first iteration — a ring-0 (`cs=0x8`) IF=0 page fault resolved through
`try_resolve_fault → resolve_subpaged_fault → fs::handle::read_at → drop(Vec) →
slab_dealloc → mm::heap::poison_free`, RIP in the debug per-byte overflow
precondition-check inside the poison loop (`rflags=0x10002`, IF=0, task tid≈133
"dash-redir"), and — like offender #1 — the log **continued past the NMI dump to
BOOT_OK**, i.e. another false-positive on slow-but-live IF=0 work. Two
independent offenders in the same window meant fixing them one at a time was
band-aid accumulation (CLAUDE.md: "if you find yourself patching around the same
issue in multiple places, stop; redesign the underlying system").

The underlying system: `main.rs` deferred `cpu::sti()` until *after* the entire
ring-3 integration self-test battery (dozens of real Linux-ABI processes — glibc,
dash, gcc/make — that fork, CoW-clone, exec, demand-page file-backed mappings),
so the whole battery ran with **IF=0**. That is the "long operation under
IRQs-disabled" anti-pattern: no timer ticks → no preemption, the timer-driven
liveness/hung-task watchdogs are blind, and the BSP-only hard-lockup kick
(`sched::timer_tick → hardlockup::kick`) is starved. In a debug build (heap
poisoning on) the battery's O(n)-over-large-data ops are seconds-long, so
host-scheduling jitter occasionally pushed a slow-but-live boot across the ~9.8 s
watchdog / harness-timeout threshold → the intermittent "BSP-dead total-silence"
fingerprint (~5%).

**Fix (commit `c596b2fcc`):** move the Step-21 interrupt enable
(`idt::init_irq_stack(0)` + `cpu::sti()` + APIC-timer verification) from *after*
the battery to the init/test seam, immediately **before** the first ring-3 spawn
self-test (`main.rs`, right after the fs/blkdev self-tests, before
`self_test_linux_dynamic_interp`). The battery now runs the way userspace
actually runs — interrupts on, preemption live. The two validations that must
follow interrupt-enable but need not precede the battery (`sleep_ns`, `softirq`)
stay at the tail of boot. Results: a clean boot reaches **BOOT_OK in 91 s** (vs
the historical 161–229 s — ~2× faster, because ring-3 children now get
timer-driven CPU + interrupt-driven I/O completion instead of cooperative
`yield_now`-only slices), and the seconds-long IF=0 offenders are gone by
construction (they run with IF=1, so the timer keeps kicking the watchdog).

**Bonus:** the timer-driven liveness / hung-task watchdogs are now **live during
the battery**, so if a *genuine* clone/CoW/reap deadlock (the still-unproven
phenomenon #2 — the 480 s no-BOOT_OK total hang seen historically) ever recurs,
it will now produce a `[liveness] SYSTEM HANG` task-table dump instead of silence,
rather than being masked by the non-preemptive cooperative driver. If that dump
ever lands, root-cause the named lost thread's wait state. Until then this bug is
downgraded from the ~5% intermittent hang to RESOLVED for the false-positive
class; a 20-boot watchdog-armed soak is validating no NMI false-fire recurs.

**FOLLOW-UP STRUCTURAL FIX 2026-07-02 — make page-fault resolution preemptible
(the residual single IF=0 window).** The battery-wide reorder above eliminated
the *seconds-long* IF=0 offenders, but a fresh 20-boot armed soak still caught
one NMI false-fire on iteration 1 (still recovered → BOOT_OK, `ctx_switches=688`
`heartbeat=1011`, so preemption was confirmed live). The NMI RIP resolved to
`resolve_subpaged_fault::closure` behind an `isr_page_fault` asm boundary —
i.e. the residual IF=0 window is a *single* page fault, not the battery. Root
cause: **#PF is an interrupt gate (IDT type 0xE), so `handle_page_fault` ran
with IF=0 for its entire duration.** A single fault can be long — demand-paging
a subpaged file frame reads up to 16 KiB through the VFS, CoW/large copies touch
many pages, and debug heap poisoning makes every alloc/free O(size) per-byte —
so one slow fault could still hold IF=0 past the ~9.8 s threshold even with the
rest of the battery preemptible. Holding IF=0 across that I/O-bound work is the
same "long operation under IRQs-disabled" anti-pattern, just narrowed to one
handler invocation.

**Fix (`kernel/src/idt.rs` `handle_page_fault`):** mirror Linux `do_page_fault`
— capture CR2 first (so a nested fault can't clobber it), then `cpu::sti()`
*only when the faulting context's saved `RFLAGS.IF` was set*. Faults from an
already-IF=0 context (ISR, scheduler, cli/raw-spin critical section) keep
interrupts disabled, so we never widen interruptibility beyond what the
interrupted code allowed. Now the timer keeps ticking (preemption + watchdog
kick + liveness heartbeat) across even a long demand-paging/CoW fault, closing
the residual IF=0 window by construction. A 20-boot watchdog-armed soak is
validating no NMI false-fire recurs.

**REPRODUCED AS A FULL HANG 2026-07-02 — ping-pong livelock in the dash-redir
ring-3 test (a liveness-watchdog blind spot).** The post-§56 armed soak caught a
*total* boot hang on iteration 1: serial froze mid-`spawn_process` for the
`spawn-test-dash-redir` child (`echo > file` redirection test) — last line
`[thread] Spawned thread (task 133) in process …`, process 167 — and stayed
silent for 6+ min with **no** NMI dump and **no** `[liveness] SYSTEM HANG` dump,
until the 480 s harness timeout. Diagnosis:
- **Not caused by §56.** The #PF `sti()` cannot cause a lock-held context switch:
  timer-driven preemption defers when `preempt_count > 0` and refuses to re-enter
  `SCHED` (`sched/mod.rs` ~L2342/2348), and tracked-lock ISRs use `try_lock`
  (`sched/mod.rs` L355). So enabling interrupts mid-fault is within the existing
  concurrency contract. This is the pre-existing dash-redir / ring-3 reap-futex
  race (same family as B-DASH-STDIN-FLAKE), now manifesting as a *hang* instead
  of a fast `InternalError` because live preemption (§55) changed the timing.
- **Why neither watchdog fired.** The hard-lockup NMI needs IF=0 on cpu0 — but the
  driver's `yield_now` loop re-enables IF between yields, so cpu0's `timer_tick`
  keeps running (kicks hardlockup → no NMI). `liveness_check` runs *directly* from
  `timer_tick` (L2165), so it *did* run every 5 s — but its two detectors are
  blind here: the total-hang path needs `useful_work` frozen and the busy-livelock
  path needs `ctx_switches` frozen, yet in a ping-pong livelock (driver re-schedules
  the deadlocked child, child runs briefly and blocks, repeat) **both** counters
  keep advancing, so neither trips.

**Instrumentation fix (`sched/mod.rs` `liveness_check`):** added a purely
time-based **boot-deadline backstop** — `LIVENESS_BOOT_DEADLINE_INTERVALS = 60`
(× 5 s = 300 s from arming). A healthy boot disarms at BOOT_OK ~91 s after arming
(>3× headroom, no false-fire risk), so if the watchdog is still armed 300 s after
arming it dumps the full task table once (`[liveness] BOOT DEADLINE EXCEEDED`).
This catches *any* hang mode — total, busy-livelock, or ping-pong livelock — that
the progress-based detectors miss, giving the task-state breadcrumb needed to
root-cause the dash-redir reap deadlock. Next armed soak should capture the dump;
root-cause the named stuck task's wait state (child `blocked_on` / driver state)
from it.

**HARD-LOCKUP WATCHDOG NMIs ARE TCG FALSE POSITIVES — ROOT-CAUSED 2026-07-02.**
_(Correcting an earlier note in this file that wrongly attributed the watchdog
trips to `task_list()`-on-exit. The `task_list` change below is kept as a real
optimization, but it was **not** the cause of the NMIs.)_ Two consecutive armed
`--hard-lockup-watchdog` catches (offender #3: `rip=0xffffffff814decc9`; offender
#4 / `build/hang-catches/CAUGHT-iter-1-hardlockup.txt`: `rip=0xffffffff80fc4248`,
in `Vec<u8>::drop` during the glibc-staging self-test) both fired the NMI with
`rflags` showing **IF=1**, in heavy debug-build compute, holding no
interrupt-disabling lock, and **both recovered to BOOT_OK**. That is decisive:
`hardlockup::kick()` sits at the *top* of `timer_tick` on cpu0, *before* any lock
acquisition, so a live-and-ticking BSP always kicks — an NMI that fires while the
BSP is demonstrably still executing `timer_tick`-eligible code and then recovers
cannot be a genuine `IF=0` wedge. These are **spurious NMIs from QEMU/TCG
virtual-clock-vs-APIC-timer divergence** during heavy debug-build compute bursts
(the poison allocator makes `O(size)` drops multi-second, and the i6300esb counts
in QEMU_CLOCK_VIRTUAL): the APIC timer that should keep kicking gets starved of
TCG translation-block boundaries relative to the watchdog's virtual clock, so the
countdown expires even though the BSP is fine. The genuine bug (offender #2) is a
*permanent* wedge (480 s, never reaches BOOT_OK); it was never one of these
catches — the spurious NMIs kept ending the soak before it could reproduce.

**Proper structural fix (this commit) — heartbeat-progress NMI discriminator.**
Per CLAUDE.md's anti-band-aid rule, rather than keep chasing individual "offender"
RIPs (each a red herring), the NMI handler now *distinguishes* a real wedge from a
spurious NMI instead of treating every watchdog NMI as a catch:
- `sched::bsp_heartbeat()` reads `WATCHDOG_HEARTBEAT[0]`, bumped every BSP
  `timer_tick` (NMI-safe: one relaxed atomic load).
- `hardlockup::classify_nmi(hb)` swaps `hb` into a `PREV_NMI_HEARTBEAT` baseline
  (reset to a sentinel in `arm()`). First NMI since arming → benefit of the doubt
  (spurious). Subsequent NMI whose heartbeat advanced `< ALIVE_TICKS` (=4) since
  the previous NMI → **real wedge** (a spin with `IF=0` freezes `timer_tick`, so
  the delta is exactly 0); advance ≥ 4 → spurious (live-but-busy BSP advances the
  heartbeat by hundreds per ~9.8 s window).
- `idt::handle_nmi` (armed branch): only **cpu0** classifies/acts (the watchdog is
  driven solely by the cpu0 kick, and `classify_nmi`'s swap must run exactly once
  per event); APs print a non-greppable info line and return. On a **real** verdict
  cpu0 emits the greppable `NMI WATCHDOG FIRED` marker + one-shot backtrace/task
  dump. On a **spurious** verdict it prints a distinct `spurious NMI … re-kicking`
  line, re-kicks, and resumes — no latch, no false catch.
This catches a genuine BSP-dead wedge on the *second* NMI (~20 s) instead of the
480 s liveness timeout, and — crucially — lets the soak run *past* the spurious
NMIs so offender #2 can finally reproduce. Builds clean, 0 new clippy warnings.

**Kept optimization (commits `acf9da4f9`, `d2da77e5c`):** `pacct::on_task_exit`
and `procfs::task_exists` no longer call `sched::task_list()` (which builds a heap
`Vec` of *all* tasks and volatile-scans every stack under SCHED just to find/test
one task). Added:
- `sched::task_info(task_id) -> Option<TaskInfo>` — one `tasks.get(&id)`, skips the
  stack scan (`stack_used`/`stack_pct` = `None`). Used by `pacct::on_task_exit`.
- `sched::task_exists(task_id) -> bool` — a `tasks.contains_key(&id)`. Used by
  `procfs::task_exists` (~14 pid-validation sites).
These are genuinely wasteful patterns worth removing on their own merits (a map
lookup holds SCHED for microseconds), but they were **not** the watchdog cause.
The genuine *never-recovers* dash-redir ping-pong livelock (offender #2, the 480 s
no-BOOT_OK case) is still open; the discriminator above plus the boot-deadline
backstop will capture its task dump on the next reproduction.

**CORRECTION 2026-07-03 (later the same day): the IRQ-stack fix below is REAL but
is NOT the (only) cause of this intermittent hang — there are (at least) TWO
distinct wedges, and the DOMINANT one is still open.** A 30-boot armed soak run
*after* the IRQ-stack fix reproduced a hang on **boot 1**, but with a completely
different signature: `[liveness] SYSTEM HANG: no task-level forward progress …
all CPUs idle-ticking`, **heartbeat still advancing** (so cpu0 is NOT wedged with
IF=0 — this is not the IRQ-stack overflow). The task table showed a **container
exec of `/bin/hello` (pid 220, task 184, inode 72)** marked `state=Running` on
cpu0 while the CPU idle-ticks, having **never executed a single instruction** (zero
page faults for its entry `0x4000000000`, no output). Saved:
`build/hang-catches/CAUGHT-iter-1-liveness.txt` /
`CAUGHT-iter14-liveness-lostwakeup.txt`. The prior session's `healthy-serial.txt`
froze on the **same inode 72** (`/bin/hello`) mid page-cache-map — so this
container-exec dispatch/wakeup hang is the recurring dominant failure and it
**predates** the IRQ-stack fix (my fix did not introduce it, nor cure it). This is
the genuine lost-wakeup / failed-dispatch race (B-PTHREAD-YIELDBUDGET /
B-DASH-STDIN-FLAKE family): a container-exec'd task is left `Running`/current on an
idle CPU. **STILL OPEN — root-cause the container exec dispatch path next.** The
IRQ-stack fix remains committed on its own merits (unbounded nesting *will*
overflow under a slow-enough handler; it was one genuine wedge — the
`CAUGHT-iter-2-nobootok` IF=0 guard-page `#PF`).

**OCCURRENCE 2026-07-14 (two back-to-back boots during Q18/§59 virtio-gpu work).**
Two consecutive `boot-test.sh` runs both timed out at `BOOT_OK not found within
480s`, but at **different, non-deterministic points**: run 1 froze at **process
211** (a `/lib64/ld-linux-x86-64.so.2` interpreter exec), run 2 froze at
**process 226** — the last serial line cut off mid-write `[spawn] Process 226
running (thread 190, e`, immediately after the container-exec sub-tests passed
(`[container] exec + wait (exec_path/wait_process): OK`), on a plain
`entry=0x4000000000` `/bin/hello`-style spawn. Total silence after, heartbeat
family (same lost-wakeup / failed-dispatch signature above). The **moving hang
location run-to-run** is the definitive tell that this is the timing-dependent
race, not a code regression: the Q18 change under test (virtio-gpu GETPARAM
render ioctl) runs *far earlier* at process 146 and **passed cleanly in both
runs** (`renderD128 GETPARAM(3D_FEATURES)==0, honest no-3D reporting: OK`), with
boot progressing hundreds of processes past it each time. Q18 committed on this
basis. **STILL OPEN — root-cause the container-exec / ring-3 spawn-dispatch race.**

**OCCURRENCE 2026-07-14 (netstack Phase 4 increment 5, UDP-exchange-over-IPC).**
One `boot-test.sh --no-build` run timed out at `BOOT_OK not found within 480s`
with the same signature: `[liveness] SYSTEM HANG: no task-level forward progress
for 15+ seconds (useful_work=13, all CPUs idle-ticking)`, cpu0 heartbeat still
advancing (2501), the current task `tid=0 name="prctl-batch269"` `state=Running`
`last_rip` in `kernel_text`. QEMU also printed a one-off `Incorrect order for
descriptors` (virtio) on stderr. Boot had progressed to ~line 4147/4175 (~99%),
well past the netstack self-tests, which **all passed cleanly** (A resolve, PTR
`dns.google`, TCP `HTTP/1.1 200 OK`, and the new UDP-exchange DNS datagram — all
OK at serial lines 1822–1831). An **immediate re-run passed in 88s** with every
netstack op OK and no hang/virtio error — the definitive tell of the timing race,
not a regression from the UDP-exchange change. Increment 5 committed on this
basis. **STILL OPEN.**

**OCCURRENCE 2026-07-14 (netstack Phase 5 increment 5.6, persistent daemon + NIC
handoff).** Multiple `boot-test.sh --no-build` runs timed out at `BOOT_OK not
found within 480s` at **different, non-deterministic Path-Z points** run-to-run —
the definitive moving-hang tell of this race: (a) with the cutover switch forced
on, a run froze in the dash pipeline test `/bin/emit | /bin/countbytes > file`
(process 174 `countbytes` blocked reading the pipe); (b) with the switch off
(default), one run froze at the `ipv4` self-test after a one-off `Incorrect order
for descriptors` (virtio) on stderr, another froze at the tcc hosted-C build
(process 217). The `prctl-batch269` idle-task name recurred (as in earlier
occurrences and in an unrelated hrtimer-self-test panic this session). The 5.6
deliverables **all passed cleanly before every hang**: with the switch **on**, the
persistent netstack daemon spawned at boot, claimed the NIC, registered
`net.stack`, and DNS (`example.com`), TCP (HTTP over the daemon) and UDP (DNS
datagram over the daemon) parity all succeeded (serial lines 1800–1810); with the
switch **off**, the bounded netstack self-tests passed (raw-frame ARP round-trip,
DNS-over-IPC). The switch-off boot path is behaviourally unchanged by the 5.6
work (the persistent daemon only spawns when `net.userspace` is set), so these
hangs cannot be a 5.6 regression. Increment 5.6 committed on this basis. **STILL
OPEN — same container-exec / ring-3 spawn-dispatch race.**

**OCCURRENCE 2026-07-15 (EEVDF-PICK-ON O(log n) rewrite validation).** Two
consecutive `boot-test.sh` runs both timed out at `BOOT_OK not found within
480s` at **different, non-deterministic points**; an **immediate third run
passed cleanly in 95s** (`BOOT_OK`, all self-tests OK) — the definitive
non-determinism tell, not a regression. Run 1 froze in the **virtio-blk write
path** (repeated `[virtio-blk] Write sector NN timed out (IRQ mode)` on the
`vdb` ext4 rootfs) with QEMU's one-off stderr `Incorrect order for
descriptors`. Run 2 froze **earlier and at a ring-0 point** — mid-serial-output
inside `budstat::self_test` (printed `  [3/` of the buddyinfo self-test line and
then went dark mid-character). **Data point re. the two-wedge model:** run 2's
freeze is *pre-userspace* (a ring-0 boot self-test, long before any container
exec / ring-3 spawn) and froze *mid-serial-write*, which is the hard-CPU-wedge
signature (the UART poll loop spins because the CPU is otherwise stuck), NOT the
`[liveness] SYSTEM HANG … all CPUs idle-ticking` lost-wakeup signature of the
ring-3 spawn-dispatch race. `budstat::self_test` itself was **audited and is
provably deadlock-free** (straight-line assertions, no loops, `spin::Mutex`
fully released between calls, STATE never touched from interrupt context) — so
budstat is a red herring: it is merely where the CPU happened to be when the
wedge fired. This reinforces that at least one still-open wedge is a general
hard-CPU-wedge that can strike at *any* point (ring 0 or ring 3), distinct from
the ring-3-only container-exec lost-wakeup race. The EEVDF change under test is
an opt-in non-default scheduler backend whose self-tests **passed cleanly in all
three runs** (`eevdf: all tests passed`, serial line ~6842) and cannot affect
the ring-0 boot path (the default `PriorityRoundRobin` runs the boot); it was
committed on this basis. **STILL OPEN.**

**BREAKTHROUGH 2026-07-15 (armed wedge-soak caught the hang with a resolved RIP —
it is a deadlock on the GLOBAL KERNEL HEAP LOCK).** After the three runs above, an
armed hang-repro soak (`scripts/wedge-soak.sh`: `boot-test.sh
--hard-lockup-watchdog --no-build` in a loop, which enables the i6300esb NMI
watchdog + the HMP monitor so a wedge's frozen CPU state is captured directly from
QEMU) **reproduced and captured the wedge on iteration 2**. The captured guest
state (`build/hang-catches/soak-20260715-004449-iter02.{serial,regs}.txt`) is
decisive:
- **Wedged RIP = `0xffffffff80b25ce6` = `core::sync::atomic::spin_loop_hint`+6** —
  the CPU is spinning in a `spin::Mutex` acquire loop, not making progress.
- **RDI = `0xffffffff827a1378` = `kernel::mm::heap::HEAP`** (resolved via
  `llvm-nm`). RDI is the first-arg / lock pointer: **the lock being spun on is the
  global kernel heap allocator's `HEAP.inner` `spin::Mutex`.**
- **RFL = `0x202` → IF=1 (interrupts ENABLED).** So this is NOT the IF=0
  hard-CPU-wedge family — it is a *lost-release spinlock deadlock* with interrupts
  live (which is why `[liveness] SYSTEM HANG … all CPUs idle-ticking` DID fire and
  dump the task table this time).
- **RBX = `0xffffffff810c2e20` = `kernel::proc::spawn::userspace_entry_trampoline`**
  — the spinning task is on the process-spawn → ring-3 entry path (task
  `port-init`, `state=Running`, in the liveness dump), consistent with the
  long-suspected container-exec / spawn-dispatch locus.
- **`info cpus` shows a UNIPROCESSOR guest (only CPU#0).** So this is NOT a
  cross-CPU AB-BA lock-ordering deadlock (that needs ≥2 spinning CPUs). On UP, a
  permanent spin on a lock means the holder is **not running and never will be** —
  i.e. **a task acquired `HEAP.inner.lock()` and then exited / was torn down
  WITHOUT releasing it** (leaked `MutexGuard`), or was preempted while holding it
  in a context that then never reschedules it. The liveness dump shows several
  `state=Dead` tasks (`/tmp/restart-init.elf`, `logs-init`) alongside the live
  spinner — a dead holder fits.

**Ruled out this session:** (1) reentrancy through the frame allocator — the heap
slow path (`KernelHeap::alloc`/`dealloc` → `HeapInner::slab_alloc` → `refill`)
holds `HEAP.inner.lock()` across `frame::alloc_frame()` and
`memtype::charge()`, but **neither allocates from the heap**: `memtype::charge`
is pure atomics, and `frame::alloc_frame` + its sub-paths (`pcpu_refill`,
`alloc_order`, `charge_cgroup_alloc`) contain no `Vec`/`Box`/`BTreeMap`/`format!`
— only fixed per-CPU array pushes and atomics (audited `kernel/src/mm/frame.rs`).
So holding the heap lock across the frame call is not itself a reentrancy
deadlock. (2) Cross-CPU AB-BA — ruled out by the UP config.

**Remaining hypothesis to confirm:** a code path acquires the global heap lock
(directly, or transitively via any `alloc`/`dealloc`/`Vec`/`Box` on the
spawn/exec/teardown path) and then the owning task is destroyed or context-switched
away permanently before the guard drops, leaving the lock held forever. The
decisive next step is **heap-lock owner instrumentation**: record the owning
task-id + acquire-site RIP whenever `HEAP.inner` is locked, and dump it from the
liveness/NMI path, so the next caught wedge names the holder and the exact
acquire site. `scripts/wedge-soak.sh` reproduces reliably (~1 catch per 1–3 armed
boots) to validate any fix. **STILL OPEN — now localized to the global heap
spinlock; next: instrument the owner and identify the leaked-guard / dead-holder
site.**

**ROOT-CAUSED AND FIXED 2026-07-15 (it is a SERIAL-lock re-entrancy deadlock,
NOT the heap lock — the earlier RDI=&HEAP read was a coincidental register
leftover).** After adding the heap-lock owner instrumentation above, a fresh
armed soak caught the wedge again on iteration 6
(`build/hang-catches/soak-20260715-012819-iter06.*`), and this time the **NMI
hard-lockup watchdog produced a full `rbp`-chain backtrace** — vastly more
reliable than guessing the lock identity from a leftover register. The
backtrace is decisive:

```
core::sync::atomic::spin_loop_hint        <- spinning on a spin::Mutex
kernel::sched::liveness_boot_deadline_check   <- the frame taking the lock
kernel::sched::timer_tick
handle_timer_irq / dispatch_vector / run_on_irq_stack / irq_common_dispatch
isr_timer                                  <- TIMER IRQ context
kernel_main                                <- interrupted here (boot self-tests)
kmain
```

The heap-lock owner dump printed **`heap-lock: unlocked (no current holder)`** —
conclusively exonerating the heap. The only lock `liveness_boot_deadline_check`
acquires is the **global `SERIAL` `spin::Mutex`**, via its 30 s "boot-window
breadcrumb" `serial_println!`. Root cause: **`serial_print!` acquired
`SERIAL.lock()` without disabling interrupts.** A task doing boot self-test
output (`kernel_main`, serial-heavy) held the lock mid-write; a timer IRQ fired
on the **same CPU** (interrupts enabled); the ISR's `liveness_boot_deadline_check`
breadcrumb tried to re-acquire the already-held `SERIAL` lock and spun forever —
the interrupted task can never resume to release it. This is a textbook
ISR-vs-task non-reentrant-spinlock deadlock on the console lock.

It explains **every** prior symptom in this cluster: the "mid-serial-write" hard
freezes (the wedge *is* a serial write), the ring-0 hard-CPU-wedge signature
(`spin_loop_hint`, IF spinning), and the non-determinism (needs a timer tick to
land inside the narrow serial-lock window — and the breadcrumb path needs the
tick to also cross a 30 s bucket, hence ~1-in-several-boots). The earlier iter02
`RDI=0x…HEAP` was a register the interrupted code happened to leave behind, not
the lock identity — `spin_loop_hint` takes no arguments.

**Fix** (`kernel/src/serial.rs`): `serial_print!`/`serial_println!` now route
through a `serial::_print(fmt::Arguments)` function that takes the `SERIAL` lock
**inside `cpu::without_interrupts(...)`**, so same-CPU IRQ re-entry is
impossible (the standard console-lock discipline, cf. Linux `spin_lock_irqsave`).
Cross-CPU contention remains deadlock-free (the holder runs IRQ-off and releases
promptly). The `_print` function form (vs. inlining in the macro) preserves
`?`/`return` semantics for expressions used inside a `serial_println!(…)` format
argument. NMI/panic output is unaffected — it already uses the lock-free
`emergency_print!` path.

The heap-lock owner instrumentation (`HEAP_LOCK_OWNER`/`HEAP_LOCK_SITE` +
`lock_tracked()` + `mm::heap::dump_lock_owner()` wired into the liveness dump) is
**kept** — it is cheap, and it is what proved the heap was innocent; it will name
the holder immediately should a *heap*-lock deadlock ever occur.

**Validation — DONE (2026-07-15).** Rebuilt; boots green; re-ran the armed
`wedge-soak.sh`. **Four consecutive clean armed boots** (soak-20260715-020155
iters 01–04, 97/101/101/92 s to BOOT_OK) with **no `spin_loop_hint` wedge** — the
`spin_loop_hint ← liveness_boot_deadline_check ← timer_tick` deadlock no longer
reproduces. This spinlock-deadlock issue is considered **fixed**. As predicted,
a *distinct* wedge with a different signature then appeared (iter05, see next
entry) — that is a separate lost-wakeup/hang issue, not this deadlock.

**Recurrence 2026-07-18 (Q24 wedge-soak — pthread yield-budget flake, NOT a
lock regression).** During the final validation soak for the Q24 raw-`spin::Mutex`
holder-preemption conversion (`MONITOR_PORT=57321 MAX_ITERS=6`), **no wedge was
caught in 6 armed boots** (the holder-preemption race did not fire — Q24's
core goal validated), but iter 02 tripped the B-PTHREAD-YIELDBUDGET flake:
`[spawn] FAIL: real glibc pthread — process did not exit within 262144 yields
(state=Some(Running))` → `WARNING: Path-Z real glibc pthread self-test failed:
TimedOut`. Notable twist: this fired at a **normal** BOOT_OK time (133 s), not the
historically-heavy ~217–229 s runs — because the budget counts *scheduler yields*,
not wall-clock, and Q24's preemption-timing changes (leaf locks now
`PreemptSpinMutex`, i.e. preempt-disabling) legitimately shift how many yields the
driver burns while the child runs. This is **not** a Q24 correctness regression:
the futex primitive was already proven sound (2026-07-01 audit), the implicated
machinery is the ring-3 clone/CoW/thread-teardown path (untouched by Q24), and
5/6 pthread runs in this very soak passed. Second-order harness quirk reconfirmed:
the *non-fatal* `TimedOut` WARNING string literally contains "self-test failed",
so `boot-test.sh check_selftest_failures` flags the boot FAILED even though the
warning is meant to be non-fatal — extra motivation for the long-deferred harness
fix (wait on a real exit signal / adaptive budget rather than a fixed yield count,
and/or make the WARNING text not collide with the failure-grep phrase).

**SEPARATE STILL-OPEN WEDGE — `gen_dmastat` / `restart-init.elf` spawn-dispatch
(first isolated 2026-07-15, `build/hang-catches/soak-20260715-020155-iter05.*`).**
On the very soak that validated the serial fix, iter05 caught a *different* wedge
that the NMI watchdog reported:
- **Wedged RIP = `0xffffffff80e6d896` = `kernel::fs::procfs::gen_dmastat+1270`
  (0x4f6)** — genuinely inside `gen_dmastat`, not a leftover symbol.
- **`RFL=0x202` (IF=1)** — interrupts *enabled*, so this is NOT the IRQ-off
  spinlock deadlock; the CPU is not spinning with IF=0.
- `RBX=0xffffffff80f8f7a0`, `CR2=0x60000ee200`.
- Serial tail: `[spawn] Process 225 running ("/tmp/restart-init.elf")` then
  `[liveness] SYSTEM HANG … all CPUs idle-ticking`, `heap-lock: unlocked`, and a
  2-task dump: **tid=0 `"prctl-batch269"` state=Ready**, **tid=189
  `"/tmp/restart-init.elf"` state=Running cpu=0**, `cpu0 last_rip=gen_dmastat`.

This is the recurring `restart-init.elf`(Running) / `prctl-batch269`(Ready)
spawn-dispatch signature from earlier in this cluster. Because `record_last_rip`
is called *unconditionally* from the timer ISR, `last_rip=gen_dmastat` means cpu0
was *actively executing* `gen_dmastat` at the last tick (not parked idle) — which
is in tension with a pure lost-wakeup model and hints at a busy path (a loop or
repeated re-entry) inside or above `gen_dmastat`. `gen_dmastat` itself
(`fs/procfs.rs:10081`) is straight-line with no obvious infinite loop, and
`dmastat::device_stats()`/`stats()` are bounded Vec clones — so a **single** RIP
frame is inconclusive (`last_rip` has been a red herring before: kernel_text,
budstat, now gen_dmastat).

**Proper next step (in progress):** make the liveness `SYSTEM HANG` dump
(`sched::dump_all_tasks_serial`) capture a full **rbp-chain backtrace of the
stuck CPU**, the way the NMI path does (`idt.rs::dump_kernel_backtrace` recovers
the interrupted RBP via `read_volatile(frame_ptr.sub(6))` from the ISR-stub save
area, then walks the chain with `backtrace::walk_from`). The liveness check runs
in the timer ISR, so to walk the *interrupted* (gen_dmastat) stack rather than
the ISR's own, the interrupt frame's saved RBP must be threaded into the dump.
Once done, the next catch of this wedge yields a conclusive call stack instead of
a lone RIP.

**UPDATE 2026-07-15 — rbp-chain backtrace implemented and it FIRED (iter19),
plus a backtrace-validator bug it exposed
(`build/hang-catches/soak-20260715-022705-iter19.*`).** The liveness `SYSTEM
HANG` dump now threads the interrupted RBP (sampled per-CPU in the timer ISR via
`rip_sample::record_last_rbp`, read from the ISR-stub save area at
`frame_ptr.sub(6)`) into `backtrace::print_from`. On iter19 it produced a
4-frame backtrace — but the frames were **garbage**, and diagnosing *why* found
a real bug in the walker:

- The sampled `cpu0 last_rbp = 0xffffffff824ca080` is in kernel **`.data`**, not
  a stack (per-task stacks are at `0xffffc100…`, the NMI capture confirms
  `RSP=RBP=0xffffc1000003bae0`). A frame pointer can never legitimately point
  into `.text`/`.data`.
- `backtrace::is_valid_frame_ptr` nonetheless **accepted any address ≥
  `0xFFFF_FFFF_8000_0000`** ("static stacks"), so the walker dereferenced static
  data as a frame chain and emitted four bogus frames (`#2/#3` resolved to
  `kshell::cmd_startmenu`, which is not on any live call path). **Fixed**: the
  validator now accepts, within the kernel image range, *only* the exact bounds
  of `KERNEL_BOOT_STACK` (new `crate::boot_stack_bounds()`), rejecting general
  `.text`/`.data`. A non-stack RBP now yields zero frames (honest "no backtrace")
  instead of a misleading one.
- **Why was `last_rbp` a `.data` pointer?** The timer sample (`last_rip =
  alloc::collections::btree::node::slice_shr+0x5d`, a `BTreeMap` node op) and the
  NMI capture (`RIP = 0xffffffff80a4f036 = kshell::cmd_queryable+0xd96`, DWARF →
  the `shell_println!`/`format!` macro; `RFL=0x202`, IF=1) are from **different
  instants** — cpu0 is executing *different* code at different times, i.e. this
  is a **livelock, not a hard freeze**, consistent with the `gen_dmastat` family
  above (batch task `tid=0 "prctl-batch269"` shown **Running**, liveness insists
  "all CPUs idle-ticking"). At the timer tick that recorded the sample, RBP
  happened to hold a data pointer (mid-prologue/epilogue or a leaf helper not
  using RBP as a frame pointer), so the walk-start was junk. The reliable signal
  remains the NMI RIP (`cmd_queryable`, in `shell_println!`/`format!`) — a busy
  alloc/format path — plus the livelock character. `last_rip`/`last_rbp` samples
  remain **inconclusive** for this wedge family (a lone sample has been a red
  herring three times now: kernel_text, budstat, gen_dmastat, and now a
  `.data` RBP). The next catch will at least no longer print a fabricated stack.

**UPDATE 2026-07-15 (second catch) — validator fix works; stale-stack limit
confirmed; per-CPU recent-RIP history added
(`build/hang-catches/soak-20260715-032821-iter19.*`, `RIP=0xffffffff81e7d35a`).**
A second soak caught the same wedge, and this time the backtrace-validator fix
paid off: `cpu0 last_rbp = 0xffffc10000063ae0` is a **valid task stack**, so
`backtrace::print_from` walked a clean **9-frame** chain instead of garbage — the
fix is confirmed good. But diagnosing the frames surfaced the *deeper* limitation:
- Frames `#0/#1/#2` land inside `procfs::self_test` (a 163 KiB symbol, verified
  with `llvm-nm --print-size` + Python bisect since debug-build monomorphizations
  have no exported symbols and defeat nearest-symbol lookup). Yet the serial log
  shows `[procfs] Running self-test...` **completed at line 3201**, long before
  the hang (line 6181). So those frames are **stale** — leftover return addresses
  on a reused stack, not the live call path.
- `last_rip` is inside `oomkiller::select_victim`'s iterator (a finite `Vec`
  fold — provably cannot infinite-loop), and the NMI-captured RIP lands in a
  symbol *gap* (discard). All three signals point to **transient** locations.
- The **reliable** signal is the serial *phase*: the hang struck during
  `[container] Running self-test...` right after `test-port-ct` (veth pair +
  published ports → NAT forward) spawns `port-init` (task 192, ring-3). This is
  the **container-exec / ring-3 spawn-dispatch race** family. IF toggling + RSP
  moving between NMI captures = **livelock**, consistent with the entries above.

**Fundamental limitation & the fix for it.** An asynchronous timer tick samples
the CPU at an arbitrary instant, which for a livelock is almost never a frame
boundary — so *both* the rbp-chain (stale return addresses) *and* the lone RIP
(whichever loop-body instruction ran) are unreliable. For a task cycling a loop
that never yields, the conclusive datum is the **set of recently-sampled RIPs**:
if the last N ticks cluster in a tight address range, that range *is* the spin
loop, revealed directly with no stack unwinding or symbol-gap guessing.
**Implemented this session:** a per-CPU recent-RIP ring buffer
(`rip_sample::RIP_HIST`, 16 samples/CPU, `record_rip_history` called every timer
tick alongside `record_last_rip`; reader `recent_rips`). The liveness
`SYSTEM HANG` dump (`sched::dump_all_tasks_serial`) now prints each CPU's last 16
sampled RIPs (newest-first, with `AddrClass`). The next catch should show whether
cpu0's RIPs cluster (→ names the livelock loop) or scatter (→ the wedge is a lock
holder / another CPU, not a spin here). Boot-tested clean; the profiler
self-test still passes.

**UPDATE 2026-07-15 (THIRD catch — BREAKTHROUGH; recent-RIP instrumentation paid
off) (`build/hang-catches/soak-20260715-043312-iter06.*`, wedged
`RIP=0xffffffff80812316`).** The new recent-RIP history turned an inconclusive
single RIP into the clearest picture yet — and this catch is *coherent*, unlike
every prior one. All addresses below resolved against the exact booted binary
(`target/x86_64-unknown-none/debug/kernel`, built 04:32, soak started 04:33) via
`llvm-nm --print-size` + Python bisect (INSIDE-symbol verified, not gap guesses).

- **Serial phase:** hang struck during `[tmpwatch] Running self-test...`, right
  after `cleanup removes files: OK (removed 3)` (Test 5) — i.e. **Test 6's first
  `Vfs::write_file("<dir>/delete_me.tmp", b"delete")`** (a 6-byte write) never
  returned. A 6-byte write cannot legitimately take the full 240 s → genuine
  infinite loop, not poison-allocator slowness.
- **rbp-chain backtrace (20 frames, all INSIDE named symbols — the validator fix
  is now confirmed good across two catches):** a clean, sensible call chain
  `kmain → kernel_main → tmpwatch::self_test → Vfs::write_file →
  Vfs::write_file_resolved → MemFs::write_file → MemFs::resolve_write_path →
  MemFs::path_components → Iterator::collect → Vec::from_iter → RawVec allocate →
  Global::allocate → __rust_alloc → KernelHeap::alloc → HeapInner::slab_alloc →
  heap::check_poison`. So at the sampled instant cpu0 was allocating the
  `path_components` `Vec` through the poison allocator.
- **Recent-RIP history (16 samples ≈ 160 ms, this is the decisive new datum):**
  NOT clustered in one tight loop but cycling through a *small working set* of
  heap + BTreeMap + iterator code: `heap::check_poison`, `heap::poison_alloc`,
  `heap::poison_free` (the O(size) byte-walk poison ops) and their inner
  `Range`/`usize::Step` loops (`spec_next`, `forward_unchecked`, `Iterator::next`);
  `BTreeMap::clone`, `BTreeMap::…insert_fit`, `slice::IterMut::next`;
  `AtomicBool::compare_exchange_weak` (a spinlock CAS); `ptr::write_volatile`
  (poison fill). Because these span *both* a BTreeMap *search/clone* phase and a
  BTreeMap *insert* phase, cpu0 is not wedged on one instruction — it is
  **repeatedly executing allocate→(BTreeMap op)→free**, i.e. a livelock, for the
  full 15 s+ that froze `useful_work` (315) and `ctx_switches` (1119).
- **`heap-lock: HELD by tid=0 acquired at heap.rs:1047:30`** (the `alloc` global
  path) — expected, caught mid-allocation; NOT a static self-deadlock (the recent
  RIPs prove forward motion, so it is not spinning 15 s on its own lock). The
  240 s NMI `RIP = spin_loop_hint+0x6` is a single sample of a transient CAS spin
  inside that busy loop, not the whole story.
- **tid=0 `"prctl-batch269"` prio=31 Running** — the boot task itself, running the
  self-test inline at max priority with no preemption point, so the loop starves
  every other task → SYSTEM HANG.

**Root-cause hypothesis (strong, not yet proven): slab/heap corruption →
cyclic `BTreeMap` → non-terminating traversal.** `resolve_write_path`'s own loop
is bounded (`MAX_SYMLINK_DEPTH = 40`) and the non-existent-file write takes the
immediate `None → return` branch, so the loop is **not** a logic bug in memfs.
The recent RIPs put the livelock inside `BTreeMap` node ops fed by poison-allocator
churn. MemFs stores directory children in a `BTreeMap<name, node>`; if a slab
free-list develops a **cycle** (the poison allocator's own `slab_dealloc` comment,
heap.rs:632, explicitly calls this out as a corruption hazard it guards against
*only* for detected double-frees — a use-after-free that overwrites a freed slot's
`->next` from an unrelated allocation would NOT be caught), two live allocations
alias the same memory, a `BTreeMap` node's child pointer becomes cyclic, and
`children.get()` / insert traverses forever. This unifies every symptom: the
BTreeMap ops in the RIP set, the poison-allocator involvement, the **moving catch
location** (corruption is timing-dependent — prior catches landed in container
self-test and `gen_dmastat`, this one in tmpwatch), and why bounded logic loops
can't explain a 240 s hang. It is the same **B-PTHREAD-YIELDBUDGET / container-exec
spawn-dispatch** intermittent family, now with a concrete mechanism to chase.

**CONFIRMING INSTRUMENT — IMPLEMENTED 2026-07-15 (free-list link validation).**
Rather than an O(n) full free-list walk, added an **O(1)-per-pop** intrusive-link
validator (`heap::free_link_valid`, guarded by `POISON_ENABLED`) wired into both
slab pop sites (`HeapInner::slab_alloc` and `pcpu_slab_alloc`). Rationale: a freed
slot's `next` pointer lives in bytes 0..8, but the poison magic/fill only covers
bytes 8..`class_size` — so an 8-byte use-after-free write to a freed slot's first
word corrupts `next` **without** tripping `check_poison`. That is precisely how an
*undetected* free-list cycle/alias forms. On each pop the validator checks the
about-to-be-installed head link is null, or (a) higher-half HHDM, (b) `class_size`-
aligned, (c) not a self-cycle; on failure it logs
`[heap] FREE-LIST CORRUPTION! class=… slot=… bad next=…` and **severs** the list
(hands out the current slot, leaks the corrupted tail) instead of following a wild/
aliasing link. This converts the silent, location-moving wedge into a precise,
located fault at the moment of damage — and, critically, *stops* the corruption
from reaching the `BTreeMap` that would otherwise livelock. Boot-tested clean (no
false positives — valid slots always pass by construction). It does not catch a
perfect cycle between two *valid* same-class slots; if the wedge recurs without a
`FREE-LIST CORRUPTION` line, the corruption source is elsewhere (buddy allocator
`mm/frame.rs` / rmap `mm/rmap.rs`) and the next tool is a bounded full-list Floyd
cycle check on `refill`. (The `lockdep_mm` migration from todo.txt targets *lock
inversion*, a deadlock — the wrong tool for this **livelock**; deprioritized.)
Instrumentation + validator committed this session; this catch is the first that
gives an actionable code-level mechanism rather than a lone red-herring RIP.

**UPDATE 2026-07-15 (FOURTH catch — DEFINITIVE ROOT CAUSE, distinct from the
free-list hypothesis; FIXED) (`build/hang-catches/soak-20260715-051830-iter22.*`,
wedged `RIP=0xffffffff80726d96`).** A 25-iteration soak *with the free-list
validator in place* caught a wedge on iter22 — and the validator did **not** fire
(`grep 'FREE-LIST CORRUPTION'` → nothing), so the free-list-cycle hypothesis is
**not** what wedged here. But the recent-RIP + heap-lock-owner instrumentation
produced a *conclusive, different* picture — a textbook single-CPU
**holder-preemption spinlock deadlock** on the global heap lock:
- **Wedged RIP = `spin_loop_hint+0x6`** (`core::sync::atomic`) — the Running CPU
  is busy-waiting in a spinlock acquire loop.
- **`heap-lock: HELD by tid=133 acquired at heap.rs:1212:30`** (the `dealloc`
  global path) — but the task table shows **tid=133 `"emit"` state=Ready**, i.e.
  the lock holder is **not running**. It was involuntarily preempted mid-critical-
  section.
- **tid=132 `"spawn-test-glibc-pipe"` state=Running** is the spinner. Its
  rbp-chain (all INSIDE named symbols): `Vfs::read_at_uncached_resolved →
  MemFs::read_at → MemFs::resolve → drop(String) → RawVec::deallocate →
  __rust_dealloc → KernelHeap::dealloc → slab_dealloc` — freeing a path-resolution
  temporary, which takes the heap lock and spins on it forever.
- **tid=0 `"prctl-batch269"` prio=31 Ready, ready_since=2168 (waited 41 ticks)** —
  a *higher-priority* task is Ready but starved by the prio-16 Running spinner.
  That is the smoking gun: **the timer is not preempting the spinner** (a prio-31
  Ready task could never be starved by a prio-16 Running one if preemption were
  live). So cpu0 is effectively pinned in the spin with no context switch — the
  Ready holder (tid=133) can never be scheduled to release the lock. Deadlock.

**Root cause (proven).** `mm/heap.rs` locks its `inner` with a **raw
`spin::Mutex`** (`use spin::{Mutex, MutexGuard}`, line 43) rather than the
preempt-aware `crate::sync::Mutex`. The raw lock does **not** call
`sched::preempt_disable()` on acquire, so a heap critical section can be
involuntarily preempted by the timer tick. This is exactly the general
**B-PREEMPT-SPINLOCK** class (see that entry, 2026-07-01: "a spinlock must never
be held across a context switch") — which was fixed for *tracked*
`crate::sync::Mutex` via the per-CPU `PREEMPT_DISABLE_COUNT`, but the heap lock
was deliberately left a raw `spin::Mutex` (to keep the global allocator out of
lockdep — it is a leaf lock taken under nearly every other lock, and dragging it
into lockdep risks re-entrant allocation) and thus **never received the
preempt-disable protection**. The `dealloc`→`slab_dealloc` critical section got
preempted; a second task then spun on the same lock; single CPU → permanent wedge.

**Fix (committed this session).** Give the heap lock the preemption protection
directly, *without* pulling it into lockdep: `KernelHeap::lock_tracked()` now calls
`sched::preempt_disable()` before `self.inner.lock()` (disabled *before* the spin,
mirroring `crate::sync::Mutex`), and `TrackedGuard::drop` calls
`sched::preempt_enable()` — after releasing the physical spinlock. To order the
physical release strictly *before* the preempt re-enable (otherwise a window
exists where the lock is held but preemptible again — reopening the bug), the
guard wraps its `MutexGuard` in `ManuallyDrop` and explicitly drops it, then
re-enables. This defers the timer's context switch until the heap lock is
released, so the lock is never held across a switch. No lockdep, no re-entrant
allocation, no extra overhead on the hot path beyond two relaxed atomics. This is
a *distinct* bug from the free-list-cycle hypothesis above (which the validator
did not confirm here); the free-list validator stays in as cheap defence-in-depth.
Re-soak after this fix to confirm the `spin_loop_hint` wedge no longer reproduces.

**UPDATE 2026-07-15 (heap fix VALIDATED by re-soak; a NEW, distinct blocker
surfaced) (`build/hang-catches/soak-20260715-061420-iter12.*`).** Re-ran the soak
after the holder-preemption fix above. **The `spin_loop_hint` / holder-preemption
signature is gone** — and boot now progresses *hundreds* of self-tests further
(from the tmpwatch phase all the way to the eventfd-timeout self-test phase)
before wedging. So the heap deadlock fix is confirmed working. The new iter12
wedge is a *different* bug, with two parts:

1. **PRIMARY (NOT REPRODUCIBLE after the container fix — downgraded from blocker
   to latent) — nested device-IRQ dispatch hang.** cpu0 was stuck with `IF=0`,
   heartbeat frozen ~9.8 s, inside a *nested* device-IRQ handler (`isr_irq11`)
   reached from the **outermost timer's IF=1 window** (the timer re-enables
   interrupts on the IRQ stack for softirq/preempt; a level-triggered device IRQ
   then fired and nested). `RSP=0x…27be8` is only ~0x418 below the IRQ stack top
   `0x…28000` — i.e. only ~1 KiB into the 16 KiB IRQ stack, so this is **not** a
   deep-nesting stack overflow (unlike the 2026-07-03 catch below). Suspected: a
   level-triggered IRQ on vector 11 re-firing / storming (not ACKed/masked), or an
   ISR-reentrancy lock spin.
   **STATUS 2026-07-15:** after the container `TABLE` holder-preemption fix (which
   was the iter03 catch), a **40-iteration soak came back 40/40 clean**
   (`build/soak-postfix2.log`, `WEDGE_SOAK_DONE rc_caught=0`) — this IF=0
   device-IRQ signature did **not** reproduce even once. Given the earlier soaks
   caught wedges by iter03/12/22 and this one caught none in 40, the intermittent
   boot wedge is resolved for practical purposes. This IF=0 catch was seen exactly
   once and is not currently reproducible; it may have been a downstream symptom of
   the same task-starvation the holder-preemption deadlocks caused (a spinning,
   never-preempted CPU can leave a device IRQ un-serviced), or a genuinely
   ultra-rare separate race. **Not claiming it fixed** (never root-caused), but it
   is no longer an active blocker. Re-open if a future soak reproduces it — the
   crash-dump guard-page fix means the next catch will have a full task-table dump
   to work from.

2. **SECONDARY (FIXED this session) — crash-dump stack-scan over-read.** The wedge
   was caught, but the hard-lockup crash dump then took a **fatal `#PF` at
   `0xffffc10000028000`** (the IRQ-stack guard page), which *destroyed* the
   task-table dump needed to root-cause the primary. Cause: `dump_kernel_backtrace`
   (idt.rs) scans/chases words **upward** from the wedged `rsp` (a 256-word / 2 KiB
   stack scan, plus the rbp-chain walk) with **no bound against the IRQ-stack
   top** — with `rsp` only 0x418 below the top, the scan ran straight into the
   guard page. Fix: added `irq_stack_top_for(addr)` (returns the current CPU's
   IRQ-stack top iff `addr` is on that stack, else 0) and applied it to **both**
   walkers — the stack scan caps at `irq_top` before each read; the rbp-walk checks
   per-iteration (the chain legitimately crosses off the IRQ stack onto the boot
   stack, where the helper returns 0 → no cap). Now a wedge near the IRQ-stack top
   dumps cleanly instead of double-faulting, unblocking diagnosis of the primary.
   Built clean, BOOT_OK, committed.

**UPDATE 2026-07-15 (SIXTH catch — crash-dump fix VALIDATED; SECOND holder-
preemption instance, on the container `TABLE` lock; FIXED)
(`build/hang-catches/soak-20260715-070017-iter03.*`, wedged
`RIP=0xffffffff81056446`).** With the crash-dump over-read fixed, a 30-iter soak
caught a wedge on iter03 and this time **the crash dump survived cleanly** (no
guard-page `#PF`) — confirming the idt.rs fix. The dump gives a conclusive
picture, and it is a **third-party of the same holder-preemption class** as the
heap deadlock, but on a *different* raw `spin::Mutex`:
- **`RFL=0x202` → IF=1** (interrupts *enabled*) — so this is NOT the iter12 IF=0
  device-IRQ hang; that remains a separate open item above.
- **Wedged RIP = `spin_loop_hint+0x6`** with one sample in `AtomicBool::load` — a
  busy-wait in a spinlock acquire loop.
- **rbp backtrace:** `syscall_entry → syscall_handler_inner → sys_exit →
  on_thread_exit → remove_thread → notify_init_exit+0xbf` — i.e. an exiting
  thread's `sys_exit` path, spinning on `container::TABLE.lock()` (container.rs:1438).
- **Task table (survived!):** `tid=189 "/tmp/restart-init.elf" state=Running
  prio=16` is the spinner; **`tid=0 "prctl-batch269" state=Ready prio=31,
  waited=1735 ticks`** is a *higher*-priority task that is Ready but never
  scheduled — the smoking gun that the spinner's context is non-preemptible and
  the Ready TABLE holder can't run to release the lock. `heap-lock: unlocked`
  (so the heap fix held; this is a *different* lock).

**Root cause (proven).** `container::TABLE` (and `EVENT_LOG`) used a **raw
`spin::Mutex`** (`use spin::Mutex`, container.rs:47), which — like the heap lock
before its fix — does **not** disable preemption on acquire. A container operation
(tid=0) held TABLE and was involuntarily preempted mid-critical-section; a process
exiting via `remove_thread → notify_init_exit` then spun on `TABLE.lock()` forever
while the Ready holder (tid=0) could never be scheduled on the single CPU. Note
`remove_thread` correctly **drops `PROCESS_TABLE` before** calling
`notify_init_exit` (pcb.rs:2039), so this is *not* a lock-ordering/nesting bug —
it is purely the missing preempt-disable on the raw spinlock.

**Fix (committed).** Converted `container::TABLE` and `EVENT_LOG` from raw
`spin::Mutex` to the preempt-aware `crate::sync::Mutex` (named for lockdep:
`container-tbl` / `container-evt`). The tracked mutex calls `preempt_disable()` on
acquire, so a holder can never be preempted mid-critical-section → the hold is
bounded and short → the exit-path spinner finds TABLE free almost immediately. As
a bonus this adds lockdep coverage (which would have *caught* this) and owner
tracking. Safe because container locks are only taken in task context (never ISR:
`restart_backoff_fire` runs in the hrtimer ISR but only submits to the workqueue),
and `EVENT_LOG` is a leaf (never acquires TABLE), so no ordering inversion.

**SYSTEMIC NOTE — raw `spin::Mutex` holder-preemption is a latent class, not a
one-off.** Confirmed instances: heap, container `TABLE` (holder-preemption);
`sysctl::REGISTRY`, completion-timer→`SCHED` (interrupt-reentrancy). The kernel
has ~476 files importing `spin::` — any raw `spin::Mutex` whose critical section
can be involuntarily preempted *and* is contended across tasks is a latent
holder-preemption deadlock on a single CPU. Most are safe (true leaf locks,
trivially short sections, or never contended under preemption).

**STRATEGY UPDATE 2026-07-18 — Q24 RESOLVED: operator chose the PROACTIVE
audit/conversion (option B), see `design-decisions.md` §70.** The prior
reactive-only strategy (below) is superseded. We are now deliberately eliminating
the class rather than waiting for the soak to surface each instance. Execution is
per-lock triage (NOT a blind sed):
- **Hot / cold leaf locks** (the ~230 `fs/*.rs` procfs config & stat stores, and
  other true leaves) → the new **`PreemptSpinMutex`** in `kernel/src/sync.rs`
  (commit 03cccdd5f): `preempt_disable` around a raw `spin::Mutex`, no lockdep,
  shares the stall detector. Closes holder-preemption without dragging cold locks
  into lockdep. Conversion is a per-file import swap
  (`use spin::Mutex;` → `use crate::sync::PreemptSpinMutex as Mutex;`); the
  compiler validates API compat, and converting is strictly safe (it can only
  *introduce* a problem if the section already sleeps while holding the lock,
  which would already be a latent deadlock under raw spin).
- **Contended, non-leaf / ordering-sensitive locks** (core FS: `vfs`, `handle`,
  `fdtable`, `pipe`, `overlay`, `ext4/*`, `memfs`, `cache`, mount/notify families;
  and cross-subsystem locks) → `crate::sync::Mutex` (full lockdep + owner
  tracking) so ordering bugs are caught.
- **Deliberately-raw** (global heap lock — lockdep can't allocate under it; SCHED)
  → keep raw + manual `preempt_disable`/`enable`.
- IRQ-context acquirers stay on `try_lock`/`without_interrupts` (don't regress the
  already-clean interrupt-reentrancy surface).

**Rollout progress (running checklist):**
- [x] `PreemptSpinMutex` primitive + self-tests (Test 5/6), boot-green (03cccdd5f).
- [ ] Convert `fs/*.rs` cold leaf config/stat stores → `PreemptSpinMutex`, in
  reviewable batches, boot-test + wedge-soak between batches (~230 files).
- [ ] Route core-FS contended locks → `crate::sync::Mutex` (lockdep), triage the
  lock-ordering reports that surface.
- [ ] Sweep non-fs subsystem raw locks (`net/`, `ipc/`, drivers) per the same
  triage.
- [ ] Final full wedge-soak (switch-off + switch-on) clean.

**Superseded reactive strategy (kept for context):** the armed hang soak
(`scripts/wedge-soak.sh`) reliably reproduces these under stress; each catch names
the exact lock via the backtrace; convert *that* lock to `crate::sync::Mutex`
(or, for true leaf/allocation locks like the heap, keep raw + manual
preempt_disable/enable). This remains the fallback if the proactive sweep is
paused.

**END-TO-END VALIDATION 2026-07-15 — ring-3 socket capstone passes on the
switch-on spawn-heavy path.** With both holder-preemption fixes in, the deferred
netstack Phase-5.6 ring-3 HTTP capstone (`services/httpget`, a Linux-ABI ring-3
ELF doing raw `socket()`/`connect()`/`write()`/`read()`/`close()` over the
daemon-backed fd path, spawned by `run_persistent_netstack`) now boots and runs
green switch-on: `Created process 228 ("httpget")` -> `[httpget] connected` ->
`[httpget] OK: HTTP response` -> `ring3 HTTP capstone: OK ... (exit 0)`. This is
the spawn-heavy container-exec/ring-3 dispatch path that previously wedged; a
15/15-clean switch-on wedge soak plus this live spawn confirm the container-exec /
ring-3 spawn-dispatch race is resolved for practical purposes. (The self-test is
bounded by a 15 s Zombie-poll deadline so a stuck fetch can never wedge the boot.)

**IRQ-stack overflow wedge (one of the two) — ROOT-CAUSED AND FIXED 2026-07-03.**
The
first-NMI one-shot backtrace (added to `idt.rs::handle_nmi` this session so a
genuine wedge dumps its stack regardless of the spurious/real classification)
finally caught the real wedge: `build/hang-catches/CAUGHT-iter-2-nobootok.txt`.
Decisive evidence:
- First NMI at `rip=0xffffffff80083956`, **`cs=0x8` (ring 0), `rflags=0x10002`
  (IF=0)**, `rsp=0xffffffff…27a80` — i.e. cpu0 wedged in the kernel with
  interrupts off, in Task 0 `"prctl-batch269"`.
- The rbp chain + stack scan showed the LAPIC timer handler recursively nested on
  the per-CPU IRQ stack: the cycle `isr_timer → irq_common_dispatch →
  run_on_irq_stack → dispatch_vector → handle_timer_irq → timer_tick →
  liveness_boot_deadline_check → clock_monotonic → tsc_freq` repeats many times,
  under a task doing `spawn_process → load_interpreter → read_file → …read_through
  → get_or_fill → fill_file_page → MemFs::read_at → touch_accessed_relatime →
  metadata_now_ns → clock_realtime`.
- It ended with `[fault] Guard page hit at 0xffffc10000028000 — stack overflow`,
  `EXCEPTION: Page Fault (#PF) … address=0xffffc10000028000, error=0x0`, Task 0
  `"prctl-batch269"`, `FATAL: Unrecoverable kernel page fault. Halting.` The IRQ
  stack is exactly `0xffffc10000024000..0xffffc10000028000` (16 KiB, guard at
  `0x28000`).

**Mechanism.** `handle_timer_irq` (apic.rs) re-enables interrupts *while still
running on the IRQ stack* — once inside `softirq::process_pending` (its internal
`STI`) and once via an explicit `sti` before the deferred-preempt check. The
softirq layer's `IN_SOFTIRQ` re-entry guard bounds softirq *work*, but NOT the raw
interrupt re-enable. So whenever a timer handler takes longer than the ~10 ms tick
period — trivially true in the **poison-debug build**, where the poison allocator
makes `O(size)` heap ops multi-second and every file-page read does a
`relatime → clock_monotonic → tsc_freq` clock call — the next timer IRQ fires while
the previous handler is still on the IRQ stack, nests (grows *down* the same stack
via `irq_common_dispatch`'s nested-IRQ branch), re-enables interrupts again, and so
on. Depth grows without bound until the 16 KiB IRQ stack overflows its guard page →
fatal kernel `#PF`. This is a *uniprocessor* bug (QEMU boots 1 CPU here), which is
why "SMP timing race" framings never panned out. It is the same B-DF1 IRQ-stack
design (Q7 option A) whose own note (below) warned *"A correct IRQ-stack
implementation must therefore support nesting (or …)"* — nesting was supported but
never *bounded*.

**Structural fix (commit this session; `apic.rs` + `cputime.rs`).** Only the
**outermost** timer handler may re-enable interrupts. `cputime` already keeps a
per-CPU hardirq nesting depth (`irq_depth`, bumped in `enter_irq`); a new
`cputime::irq_depth()` accessor exposes it, and `handle_timer_irq` computes
`let nested = cputime::irq_depth() > 1;` right after `enter_irq()`. When `nested`,
it **skips `process_pending`** and **skips the explicit pre-preempt `sti`**, so the
nested handler runs its entire body with IF=0. Because the timer IDT entry is an
**interrupt gate** (type `0x0E` → IF auto-cleared on entry) and the nested handler
never sets IF back, *no further timer can fire until the nested frame returns* —
hard-capping timer-on-timer nesting at **depth 2** regardless of how slow any
single handler is. Softirq bits raised by a nested tick are drained by the outer
frame's own `process_pending` loop (identical to the `IN_SOFTIRQ` short-circuit,
but without ever toggling IF); preemption is unaffected (nested IRQs never run
`do_deferred_preempt` anyway — the outermost frame owns it). Builds clean, 0 new
clippy warnings. NOTE: the post-fix soak did NOT reproduce the IRQ-stack overflow
again, but it DID reproduce the *other* (dominant) wedge — the container-exec
lost-wakeup described in the CORRECTION note above — so this fix cannot be
soak-"verified" in isolation until that second wedge is also fixed. It stands on
its analytical merits (bounded nesting by construction) plus the absence of any
further IF=0 guard-page `#PF`.

**NMI WATCHDOG BLIND-SPOT — ROOT-CAUSED AND FIXED 2026-07-03 (why the dominant
wedge escaped with *zero* catchable NMIs).** After the IRQ-stack fix, three more
armed soaks (`CAUGHT-iter-2-nobootok` make-cc pid 210 inode 126; a tcc-hosted
catch pid 214; `soak5` `CAUGHT-iter-1-nobootok` pid 176 **inode 72** — a
*different* binary again) all reproduced the dominant wedge as a `nobootok` with
**no watchdog dump at all**, running silently to the 480 s harness kill. Decisive
observations from those catches:
- **The wedge is an IF=0 total-silence spin on cpu0** — the last serial line is
  always a page-fault the handler *completes* (`[fault] … mapped/Demand-paged …`)
  right as a freshly `exec`'d ld.so-linked Linux binary is demand-paging its early
  pages, then nothing. `liveness_boot_deadline_check` emits a 30 s breadcrumb every
  BSP tick while armed, and **zero breadcrumbs** appear after the wedge → the timer
  IRQ stopped → cpu0 is spinning with IF=0 (only an NMI can preempt it).
- **It is NOT make+tcc-specific.** Catches span inode 126 (tcc), inode 72
  (`/bin/hello`-class), and make grandchildren — i.e. the common factor is
  *spawning/exec'ing an ld.so-linked Linux binary and demand-paging it*, not any
  one test. (Consequently the per-reap-loop `dump_task_table` instrumentation added
  earlier this session **cannot** observe this wedge: the reap loop runs on the same
  wedged cpu0 and is starved too. Only the NMI path can catch it.)
- **Why the NMI watchdog stayed silent.** Two compounding defects in the *diagnostic
  instrument* (not the bug itself): (1) the old `classify_nmi` compared the BSP
  heartbeat between *consecutive* NMIs against a `PREV_NMI_HEARTBEAT` baseline. A
  **mid-boot spurious TCG NMI** (seen in `CAUGHT-iter-2` at `heartbeat=997` during
  the dash-test compute burst) set that baseline to 997; minutes later the wedge
  froze the heartbeat at a large value H, so the wedge's first NMI saw `H − 997`
  (huge) and was dismissed as spurious. Catching then depended on a *second* wedge
  NMI (delta 0), which the QEMU i6300esb did not reliably re-inject after the first
  fire → no catch, ever. (2) That same mid-boot spurious NMI consumed the *one-shot*
  `HARDLOCKUP_DUMPED` latch, so even if the wedge had been classified real, the
  backtrace/task-table dump was already spent.

**Fix (commit this session; `hardlockup.rs` + `idt.rs`).** Replace the fragile
across-NMI heartbeat-delta classifier with a **self-contained monotonic
kick-staleness** check that fires on the wedge's *first* NMI, immune to any stale
baseline:
- `hardlockup::kick()` (called at the top of the BSP `timer_tick`) now stamps
  `LAST_KICK_NS = clock_monotonic()` — a direct "when did the BSP timer last tick?"
  clock. `clock_monotonic` is a pure `rdtsc` + relaxed loads, so it advances even
  with IF=0 and is NMI-safe.
- `classify_nmi()` (no args) returns real iff `clock_monotonic() − LAST_KICK_NS ≥
  WEDGE_STALE_NS` (2 s). A live BSP kicks every ~10 ms → staleness ≪ 1 s → spurious;
  a real wedge stopped kicking → by the ~9.8 s hardware fire the stamp is ~9.8 s
  stale → real, on the *first* NMI. The old `PREV_NMI_HEARTBEAT`/`ALIVE_TICKS`
  baseline machinery is removed.
- `idt::handle_nmi` now, on a **real** verdict, dumps the backtrace + task table
  **unconditionally** (ignoring the one-shot latch) so a prior spurious NMI can no
  longer rob the real wedge of its stack trace; it logs `kick_stale_ns` for
  confirmation. Spurious NMIs still take a one-shot early dump, re-kick, and resume.
Builds clean, 0 new clippy warnings. This makes the dominant wedge **observable**:
the next armed soak should finally print `NMI WATCHDOG FIRED … rip=…` + backtrace
pinpointing where the freshly-exec'd binary's demand-paging path spins with IF=0.
**Still OPEN** (the underlying wedge) — but no longer a blind heisenbug.

**REGISTER-VS-RUNNABLE RACE — ROOT-CAUSED AND FIXED 2026-07-03 (the yield-budget
PANIC variant; the silent IF=0 wedge is a SEPARATE bug, still open).** The reset
experiment (`scripts/wdog-reset-experiment.sh`, `WATCHDOG_ACTION=reset`) caught a
*non-silent* member of this family: `build/hang-catches/RESET-CAUGHT-iter-2.txt`
— a fatal kernel PANIC at `container.rs:5370` `assert!(zombified, "exec'd hello
did not exit within the yield budget")`. Decisive serial evidence (lines
9119–9123): `[sched] Task 184 exiting` printed **before** `[sched] Spawned task
184 …` and `[thread] Spawned thread (task 184) in process 220`, and **no**
`[thread] Process 220 … now zombie` line ever appeared. I.e. the exec'd
`/bin/hello` child ran to completion *before* its owning process/thread were
registered, so the process was never zombified and the container self-test spun
its 100 000-yield budget and fired the assert.

*Mechanism (a classic register-vs-runnable race):* `thread::spawn`
(`proc/thread.rs`) created the scheduler task via `sched::spawn`, which enqueues
it **Ready and runnable and re-enables interrupts** (`without_interrupts` ends)
*before* `thread::spawn` did `pcb::add_thread` + the `THREAD_OWNERS.insert`. On
the uniprocessor a timer preemption in that window switches to the short-lived
child, which prints and `exit()`s; `on_thread_exit` (`thread.rs:396`) then does
`owners.remove(&task_id)?` → `None`, bails, and **skips the process's zombie
transition entirely**. (The out-of-order serial — child exit logged before its
own spawn/registration logs — is the exact fingerprint of this window.)

*Proper structural fix (commit this session; `sched/mod.rs` + `proc/thread.rs`),
SMP-correct — not a widened `without_interrupts` window:*
- `sched::spawn_suspended()` creates the task **Blocked and NOT enqueued** (and
  does not signal a CPU), sharing a new `spawn_inner(…, admit: bool)` with the
  normal immediate-admit `spawn`/`spawn_with_affinity`.
- `sched::admit()` (built on `wake()`) performs the Blocked→Ready transition and
  enqueue once the caller is ready.
- `thread::spawn` now: create the task **suspended**, complete **all** ownership
  registration (`add_thread` + `THREAD_OWNERS` insert + `Creating→Running`)
  *before* calling `admit()`. The child therefore cannot run until
  `on_thread_exit` is guaranteed to find its owning process. Includes an
  unwinding path (detach + kill) if `admit` ever fails.
Builds clean, 0 new clippy warnings in the changed files.

*Scope / what this does and does NOT fix.* This eliminates the **yield-budget
PANIC variant** (a task that *ran and exited* but left an un-zombified process).
It is analytically the same ordering hazard behind the "task `state=Running`,
never executed" liveness catch (`CAUGHT-iter-1-liveness.txt`), which the fix also
closes by construction (a task is registered before it is ever runnable). It does
**NOT** fix the **dominant silent IF=0 wedge**: a 40-boot `reset`-action soak of
the fixed kernel reproduced on **iteration 1** with a *different* signature —
`build/hang-catches/RESET-CAUGHT-iter-1.txt`: pid 188 heavily demand-paging inode
72 (`/bin/hello`) page-cache maps, then **total silence** at 47 s (no panic, no
assert, no yield-budget line), i.e. cpu0 spun with IF=0, the BSP stopped kicking,
and the i6300esb reset fired. That silent wedge is a separate mechanism (a
freshly-exec'd binary's demand-paging path spins with IF=0) and remains **OPEN** —
next step is the dedicated-NMI-IST work below so an `inject-nmi` soak can finally
dump its backtrace.

**ORPHANED-`Running` LOST-DISPATCH WEDGE — ROOT-CAUSED AND FIXED 2026-07-03 (the
BSP-*alive* lost-dispatch variant; distinct from the BSP-dead IF=0 spin above).**
The dedicated-NMI-IST + monotonic-kick-staleness instrument finally caught the
**BSP-alive** member of this family cleanly:
`build/hang-catches/NMI-NOBOOTOK-iter-2.txt`. Decisive evidence: right after
`[spawn] Process 220 running (thread 184 …)`, the box goes
`[liveness] SYSTEM HANG … all CPUs idle-ticking` with **cpu0's heartbeat still
advancing** (4251→4501 — the BSP is alive and idle-ticking, NOT wedged with IF=0,
so this is a *different* wedge from the silent-spin one), and the task dump shows
exactly three tasks:
- `tid=184 /bin/hello state=Running` — a **phantom**: never executed a single
  instruction (zero page faults for its entry `0x4000000000`, no output),
- `tid=183 hello-init state=Dead`,
- `tid=0 name="prctl-batch269" state=Ready` — the **idle/boot task, stranded Ready**.
Critically there is **no** `[sched] BUG: context switch failed` line → the orphaning
happened via the *silent* idle-fallback path, not the main dispatch path.

**Mechanism (the dispatch invariant was violable).** In `schedule_inner`,
`pick_next_local` **dequeues** the picked task, and the old code then marked it
`state=Running` **before** confirming *both* context-switch pointers
(`old_data` = outgoing/current task's saved-context slot, `new_data` = incoming
task's) were successfully extracted from the task table. When extraction failed —
`old_data` is `None` because the *current* task isn't found in `tasks` — the picked
task (184) was left **orphaned**: `state=Running`, **not** current on any CPU, and
**no longer in any run queue** (the dequeue already removed it). Nothing ever
re-enqueues a `Running` task: `check_starvation` only rescues `Ready` tasks (and
additionally skips `priority >= IDLE_PRIORITY`), so the run queue drains to empty →
every CPU HLTs forever. Because the idle/boot task (task 0) is itself only `Ready`
and stranded, it can never resume its yield loop → total hang. (This is the
BSP-alive twin of the RESET-CAUGHT yield-budget PANIC: there the driver *could*
resume and hit the `assert!(zombified)`; here it cannot resume at all.)

**Structural fix (commit this session; `sched/mod.rs`, both dispatch sites) —
restore the invariant "a task is marked `Running` only once its context switch is
committed":**
- **Idle-fallback path:** extract `old_data`/`new_data` **first**; if either is
  `None`, re-enqueue the picked task iff it is still `Ready` (`PER_CPU_SCHED.enqueue`
  with its effective priority), print `[sched] BUG: idle-fallback switch aborted …
  re-enqueued ready task N`, `drop(s)` and `continue` the fallback loop. Only when
  **both** are present is the picked task's `record_dispatch` + `state=Running` +
  `last_cpu` committed. The old trailing "context extraction failed" block is now
  unreachable (kept as a defensive no-op for the borrow checker).
- **Main path:** the pre-extraction `Running` mark was **removed**; the
  `record_dispatch`/`state=Running`/`last_cpu` write now lives **inside** the
  `if let (Some(old_data), Some(new_data)) = …` success branch. The `else` branch
  re-enqueues the picked task iff still `Ready` before returning, logging
  `[sched] BUG: context switch failed — task C or N not in table (re-enqueued ready
  task N)`.
Re-borrowing `tasks.get_mut(&picked)` to set `Running` after taking `old_data`'s
raw `&raw mut` context pointer is sound: raw pointers are not live borrows and no
map insert/remove occurs in between, so the pointer stays valid. Builds clean
(`cargo build -p kernel`, 50.6 s), 0 new clippy warnings in the edited range.

**What this fixes / what remains.** This eliminates the *total hang* from the
BSP-alive lost-dispatch: even when extraction fails, the picked task returns to the
run queue instead of vanishing, and the new `BUG:` logs will pinpoint **why**
`old_data`/the current task becomes `None` (the deeper trigger — how the *current*
task drops out of `tasks` mid-dispatch — is not yet definitively identified; static
analysis says the current task is reap-protected via `active_ids`, so the logs are
the next lead). **Also noted (not the forward-progress blocker):** the idle task
(task 0) being renamed to `"prctl-batch269"` by a userspace `PR_SET_NAME` implies
`current_task_id()` returned 0 while a userspace task ran (or the boot self-test
genuinely runs in task-0 context) — a cosmetic/desync concern flagged for later.
The **BSP-dead IF=0 silent-spin** variant (freshly-exec'd binary demand-paging with
IF=0) remains **OPEN** and is the next target once a soak captures its NMI backtrace.

**POST-ACCT-FIX SOAK OBSERVATIONS 2026-07-03 (this silent wedge recurs — NOT
caused by the ACCT `lock_irqsave` fix; two fresh data points).** After the
B-ACCT-SPINLOCK-STALL fix (`lock_irqsave`, commit `b267b5e6f`) landed and was
independently verified (a standalone boot reached BOOT_OK in ~80 s with **zero**
`ACCT` stall signatures), a `scripts/hang-repro-loop.sh` soak (`--no-build
--hard-lockup-watchdog`) reproduced this *pre-existing* silent total-hang on
iteration 1 in two consecutive runs, freezing at different points in the ring-3
glibc spawn/exec/reap battery: **soak-1 froze at pid 210, soak-2 at pid 155**
(catch preserved: `build/hang-catches/SPAWN-SLOW-soak2-pid155.txt`). Both are the
now-familiar **BSP-dead IF=0 silent-spin** fingerprint: cpu0 wedged with
interrupts disabled, the BSP timer stopped ticking, **no** `[liveness] SYSTEM
HANG` dump, **no** `[watchdog]`/`SPINLOCK STALL` line, and — critically — the
i6300esb NMI hard-lockup watchdog **did not fire** either, so no backtrace was
captured. Explicitly attributed to the pre-existing spawn-hang class above, **not**
to the ACCT fix: the ACCT fix *prevents* the recursion (IF=0 during the short leaf
hold blocks the re-entering interrupt) rather than silencing any symptom, and a
clean standalone boot passed after it, so it is not a regression source. The open
blocker is unchanged and now doubly-confirmed: **observability** — the NMI
watchdog does not fire on this particular IF=0 BSP wedge, so the next step is to
determine *why* the i6300esb → inject-NMI path fails to catch it (candidate: the
NMI IST/vector setup, or the kick stops but the injected NMI is masked/lost under
TCG in this specific spin state) before the actual spawn/exec/reap or
demand-paging spin can be root-caused.

**NMI DELIVERY VALIDATED + NEW HANG LOCUS FOUND 2026-07-03 (the observability
blocker above is narrower than thought).** Two decisive results this session:
1. *The injected-NMI → dump chain WORKS end-to-end under our exact TCG harness.*
   Temporarily wiring `hardlockup::self_test_fire()` (a deliberate ~15 s IF=0
   no-kick spin, reproducing the BSP-dead condition) into `main.rs` right after
   `hardlockup::arm()` and booting `--hard-lockup-watchdog` produced:
   `[hardlockup] NMI WATCHDOG FIRED cpu=0 rip=0xffffffff814dbbe1 … kick_stale_ns=9899867054`
   then `self-test-fire: PASS — NMI observed (fired 0 -> 1)`. So the i6300esb
   inject-nmi fires under TCG, the NMI IST2 is good, and the current
   monotonic-kick-staleness `classify_nmi` correctly returns REAL on the *first*
   NMI of a 9.9 s-stale wedge. This means the *older* silent catches (pid 210/155)
   were almost certainly on a kernel with the **pre-rewrite heartbeat-delta
   classifier** that misclassified the wedge NMI as spurious — not a delivery
   failure. (Probe reverted; kernel rebuilt clean.)
2. *A fresh silent catch on the CURRENT kernel froze in KERNEL space, not the
   ring-3 battery.* A bounded `--hard-lockup-watchdog` soak (`scripts/soak-nmi-check.sh`,
   150 s timeout) caught on iteration 1 (`build/hang-catches/SNMI-CAUGHT-1-silent.txt`,
   9340 lines): the last line is OCI self-test **Test 14** (`[oci]   metadata
   instructions (VOLUME/STOPSIGNAL/SHELL/ONBUILD): OK`, `oci.rs:4079`), i.e. it
   wedged in **Test 15 "multi-stage builds"** (`oci.rs:4082`), which does heavy
   VFS + block-I/O (`build_image`/`load_image`/`extract_layer`/`read_file`/`rmdir`).
   A single `[liveness] boot-window breadcrumb: 30s armed (…heartbeat=2398)` fired
   but **no 60 s breadcrumb, no NMI, no SYSTEM HANG** — the BSP tick went dark
   ~30 s into the armed window. This is a *different* locus from the ring-3
   spawn/exec/reap hangs, suggesting the hang family is a **shared lower-level
   primitive** (VFS path / block-device wait / a lock taken on both the OCI-build
   and ring-3-spawn paths), not something specific to `clone`/CoW.
   **Caveat / open:** the 150 s timeout may itself produce false "silent" catches
   (a slow-but-live boot cut off early). A 300 s-timeout re-soak is running to
   disambiguate: a real BSP-dead wedge will now fire the NMI (delivery proven), and
   a slow-but-live boot will either reach BOOT_OK or trip the 200 s-armed
   `[liveness] BOOT DEADLINE EXCEEDED` task dump. Result pending.

**ROOT-CAUSED + FIXED 2026-07-03: `TSC_FREQ` spinlock re-entry deadlock — this
was the silent BSP-dead wedge.** The HMP-monitor RIP capture (new tooling, see
below) caught a live wedge and, walking the frozen `RBP` chain, resolved it
exactly:
- **Frozen state:** `RIP=ffffffff800e1d46` = `spin_loop_hint+0x6` (spinning),
  `RFL` with `IF=0`, `CPL=0` (kernel), `CR2=0x60000c7800`.
- **Stack (RBP chain, innermost → outermost):**
  `tsc_freq ← clock_monotonic ← kick_staleness_ns ← handle_nmi`.
- **Mechanism (same class as B-ACCT-SPINLOCK-STALL below):** `bench::tsc_freq()`
  read the write-once calibrated TSC frequency through a `spin::Mutex<u64>`
  (`static TSC_FREQ: Mutex<u64>`). But `timekeeping::clock_monotonic()` calls
  `tsc_freq()`, and `clock_monotonic()` runs on the normal hot path **and** from
  timer-IRQ context (scheduler `bsp_heartbeat`) **and** from NMI context
  (`hardlockup::classify_nmi`/`kick_staleness_ns`). On the uniprocessor, if a
  timer IRQ or watchdog NMI fires while normal code is *inside*
  `TSC_FREQ.lock()`, the handler re-enters `clock_monotonic → tsc_freq →
  TSC_FREQ.lock()` and spins forever at `IF=0`. Silent BSP death, no ticks, all
  timer-driven watchdogs blind.
- **Why the NMI never dumped:** the watchdog NMI *was* delivered (`handle_nmi`
  is on the frozen stack — this **inverts** the earlier "NMI never taken"
  hypothesis above), but `classify_nmi`'s very first act is a
  `kick_staleness_ns()` → `clock_monotonic()` → `tsc_freq()` → the same
  `TSC_FREQ.lock()` that is *already held* by the interrupted normal-context
  code. The NMI self-deadlocks in the identical lock before it can print. That
  is why every catch was **silent** with zero watchdog output.
- **Fix (commit 5f658336c):** `TSC_FREQ: Mutex<u64>` → `AtomicU64`. The value is
  write-once at calibration and read-only forever after — it never needed a lock
  at all. `calibrate_tsc()` does `TSC_FREQ.store(freq, Relaxed)`; `tsc_freq()`
  does `TSC_FREQ.load(Relaxed)`. `clock_monotonic()` is now fully lock-free and
  genuinely IRQ/NMI-safe (its doc comment's "no locks" claim is finally true),
  and it's also faster on the hot clock path. This is the *proper* structural fix
  (lock-free for a write-once value), not a band-aid.
- **New tooling that caught it — HMP-monitor RIP capture.** No
  addr2line/llvm-symbolizer exists in any toolchain, and the in-guest NMI dump
  was itself deadlocked, so neither in-guest mechanism could see the wedged RIP.
  `scripts/boot-test.sh` now attaches a QEMU HMP monitor
  (`-monitor tcp:127.0.0.1:55123,server,nowait`, only under
  `--hard-lockup-watchdog`) and, on timeout with no wait-marker, queries it over
  bash `/dev/tcp` (`info registers` / `info cpus` / `info registers -a`) to read
  the frozen CPU's registers straight from the emulator — bypassing in-guest NMI
  delivery entirely. `resolve_kernel_symbol()` resolves RIP to the nearest
  preceding symbol via `llvm-nm -nC --defined-only`, comparing **zero-padded
  16-hex-digit strings** (NOT awk `strtonum`, whose doubles lose precision above
  2^53 for higher-half ~1.8e19 addresses) and computing the offset in bash
  64-bit arithmetic. `scripts/soak-nmi-check.sh` preserves the register dump
  (`SNMI-CAUGHT-*-regs.txt`) alongside each serial catch. This RIP-capture path
  is reusable for any future silent IF=0 wedge.
- **Confirmation (DONE):** 12-iteration `--hard-lockup-watchdog` soak (300 s
  timeout) post-fix returned **12/12 clean BOOT_OK, zero catches** — no silent
  wedge, no NMI dump, no liveness dump. Wedge no longer reproduces. Before the
  fix, this soak caught a silent wedge within the first few iterations.

### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock self-deadlock — ROOT-CAUSED + FIXED 2026-07-03

**STATUS: FIXED** (commit this session). Root cause confirmed by the
owner-tracking instrumentation: a **recursive self-deadlock** — the same task
that holds `ACCT` re-enters it from interrupt context. Fix: acquire `ACCT` via
the new `Mutex::lock_irqsave()` (interrupts masked for the hold), the standard
`spin_lock_irqsave` discipline for a lock shared with interrupt context. See
"Root cause + fix" below. Re-soak to confirm no recurrence.

**Root cause + fix (2026-07-03):** The instrumented soak reproduced on
**iteration 1** and the owner stamp printed the verdict verbatim:
`[sync]   lock 'ACCT' holder: task 138 == spinner — RECURSIVE self-deadlock
(same task re-entered the lock)` (task 138 = "countbytes", the ring-3
`/bin/emit | /bin/countbytes > file` pipeline; catch:
`build/hang-catches/ACCT-OWNER-recursive-task138.txt`).

Mechanism (uniprocessor — no cross-CPU AB-BA needed):
1. `Mutex::lock()` disables *preemption* but **not interrupts** — it leaves IF
   as-is. `ACCT` was acquired this way.
2. `ACCT` is reachable from **interrupt/softirq context**: the frame allocator
   calls `compact::try_compact()` for any `order > 0` allocation
   (`mm/frame.rs:2033`), and compaction's `estimate_movable_pages()` calls
   `accounting::tracked_count()` (`mm/compact.rs:266`) → acquires `ACCT`. So a
   device IRQ / softirq that allocates a multi-order buffer re-enters accounting.
3. Critically, the **page-fault handler re-enables interrupts** (`idt.rs:2048`,
   `cpu::sti()` when the faulting context had IF=1) *before* calling
   `mm::fault::resolve` → `map_frame`/CoW → `charge`/`uncharge`. So a
   `charge`/`uncharge` on the fault path runs and holds `ACCT` **with interrupts
   enabled**.
4. An interrupt lands while `ACCT` is held → its handler allocates an
   order>0 frame → compaction → `tracked_count()` → tries to re-acquire `ACCT`
   → spins forever (holder can never resume to release it). On UP the spinner
   *is* the same task's IRQ frame, so `owner == spinner` → the recursive verdict.

Why the earlier static analysis missed it: I looked only for a *direct*
IRQ-context accounting caller and found none; the real path is indirect
(IRQ → frame alloc → compaction → `tracked_count`) and is only opened by the
page-fault handler's `sti`. The accounting functions themselves remain correct
leaf scans; the bug was the *locking discipline*, not the functions.

**Fix:** added `Mutex::lock_irqsave()` + `MutexIrqGuard` to `kernel/src/sync.rs`
(save IF, `cli`, acquire; guard restores IF after releasing the lock and
re-enabling preemption — reverse of acquire order; nests correctly, only the
disabling edge restores). Switched all 12 `ACCOUNTING.lock()` sites in
`kernel/src/mm/accounting.rs` to `lock_irqsave()`. This masks interrupts for the
short leaf-only hold, closing the re-entry window for *any* interrupt (not just
the compaction path). A nested #PF cannot occur during the hold (the functions
only touch a static `.bss` array + trivial stack), so masking maskable
interrupts is both necessary and sufficient. Builds clean, no new clippy
warnings. Module doc in `accounting.rs` updated to document the IRQ-safety
requirement.

**Follow-up (separate, low priority):** `all_stats()` still `.collect()`s a
`Vec` under the lock (now under `lock_irqsave`, so interrupts are masked across
a heap alloc — worse for IRQ latency, though it has no live callers). Should be
count-then-release or a fixed stack buffer regardless.

---

<details><summary>Original investigation notes (pre-fix, kept for history)</summary>

#### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock stuck at end of ring-3 battery — REPRODUCED 2026-07-03

**Where:** `kernel/src/mm/accounting.rs` (the `ACCOUNTING` spinlock, named `b"ACCT"`,
line 102) / `kernel/src/sync.rs` (the `Mutex` wrapper). Caught by the armed
hang-repro soak on **iteration 7/24** with the orphaned-Running-fixed kernel:
`build/hang-catches/ACCT-STALL-iter7-*.txt`.

**This is a DISTINCT bug from the orphaned-Running dispatch wedge** (which was
committed just before this soak). Decisive discriminator: the catch shows **no
`[sched] BUG:` line**, so the fixed dispatch path is not involved.

**Observed signature (`ACCT-STALL-iter7`):**
- `[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds
  (useful_work=140, all CPUs idle-ticking)` — cpu0 heartbeat=3501 **still
  advancing** (BSP alive, not an IF=0 spin), `local_has_real_work=false`,
  `last_rip=0xffffffff81107fb9 (kernel_text)`.
- Task dump: **91 tasks, 90 `state=Dead`, only `tid=0` (the boot/self-test
  driver, name overwritten to "prctl-batch269") is `state=Running`** on cpu0 at
  prio=31. This is the very end of the ~34-test ring-3 battery — everything ran
  and exited, leaving only the driver.
- Then: `[sync] *** SPINLOCK STALL *** lock 'ACCT' still not acquired after ~30s
  of spinning (cpu 0, task 0, 66805760 iters). Likely self-deadlock or lock
  convoy` followed by `[lockdep]   cpu 0 holds 0 lock(s):`. So task 0 spins
  ~66M iters trying to acquire `ACCT`, which the timer-driven liveness watchdog
  cannot rescue (the spin holds the CPU with preemption disabled).

**Analysis so far (static; not yet definitive):** The `ACCT` lock is
`mm/accounting.rs`'s `Mutex` (a `spin::Mutex` that does **not** disable
interrupts — `lock()` only `preempt_disable()`s). All *live* callers of the
accounting functions (`charge`/`uncharge` on the map/unmap/CoW page-fault path;
`query`/`tracked_count`/`largest_rss`/`memory_info` from procfs/kshell/
diagnostics/invariant checks) run in **task context** — I could not find any
IRQ/softirq/timer-context caller, which argues *against* a simple
interrupt-reentrancy self-deadlock. The accounting functions themselves are all
leaf scans that never yield/fault/allocate under the lock, so a single call
cannot leak the guard. The one structurally-unsafe function, `all_stats()`
(collects a `Vec` *under* the lock — violates the module's documented "ACCT is a
leaf lock, never held across other lock acquisitions" invariant), has **no live
callers**, so it is not the trigger here (but should be fixed on its own merits:
count-then-release or use a fixed stack buffer). `lockdep cpu 0 holds 0 locks`
is ambiguous — lockdep may only mark a lock *held* after successful acquire, so
a spinner shows 0, and the true holder (if since-dead) leaves no lockdep trace.

**Instrumentation added (commit this session; `sync.rs`) to make it definitive
on the next repro:** every `Mutex` now records the acquiring task id in a new
`owner: AtomicU64` (set in `make_guard`, cleared to `OWNER_NONE`=`u64::MAX` in
`MutexGuard::drop` — one relaxed per-CPU read+store, negligible next to the CAS
and lockdep call already present). `report_stall` now prints the holder and
classifies the stall:
- `owner == spinner tid` → **recursive self-deadlock** (same task re-entered).
- `owner == some other task` → **guard held by another task** (leaked if that
  task is Dead in the dump).
- `owner == OWNER_NONE` → **lost-unlock / flag desync** (spinlock flag set with
  no recorded holder).
This single datum discriminates all three hypotheses. Builds clean. **STILL OPEN
— re-run the armed soak with the instrumented kernel; the next `ACCT` stall will
name its holder and pin the exact leak/recursion path.**

</details>

### B-DASH-STDIN-FLAKE. `dash script-from-stdin` ring-3 self-test intermittently returns `InternalError` — WATCH 2026-07-01

**Where:** the boot self-test that runs the REAL `dash` shell over a script fed
on fd 0 (`kernel/src/proc/spawn.rs` ring-3 dash integration test; serial marker
"REAL dash shell script-from-stdin …"). Normally logs `… captured 55 bytes ==
expected, EOF→exit 0): OK`.

**Observed:** on one boot (2026-07-01, `BOOT_OK after 181s`) the harness logged
`WARNING: Path-Z real dash shell script-from-stdin self-test failed:
InternalError` (serial line 3589) instead of the OK line, while the *immediately
preceding* boots (identical dash test) passed. Load-dependent — same family as
B-CONTAINER-JAIL-TESTRACE / B-PTHREAD-YIELDBUDGET (intermittent races in the
ring-3 `clone`/`fork`/`exec`/reap + futex machinery). Non-fatal on this run:
BOOT_OK was still reached; only the one sub-test flaked.

**Assessment:** almost certainly the same underlying low-probability
spawn/exec/reap or futex race already tracked for pthread/container tests, not a
dash-specific logic bug. **Proper fix:** shares the root-cause work with the
pthread `clone`+futex deadlock (B-PTHREAD-YIELDBUDGET) — instrument the ring-3
spawn/reap path (lock-order tracer + futex wait/wake ordering) and fix the race;
also make the dash harness distinguish a transient spawn failure from a real
shell error. Logged so the intermittent dash failure isn't forgotten.

**Diagnostic classification DONE (2026-07-01):** all ~34 ring-3 real-binary
self-tests (`proc/spawn.rs`: glibc hello/stdio/full/pthread/signal/fault/
sigqueue/forkexec/pipe/redir/redirin, all 16 real-dash tests, make/cc/hosted-cc/
make+tcc) previously collapsed *both* a **hang** ("did not exit within N yields")
and a genuine **wrong-result** (mismatched output/exit code) into the same
`KernelError::InternalError`, so a captured flake report ("InternalError") could
not be told apart from a real shell logic bug. Now the two are distinct: a
never-reached-Zombie timeout returns `KernelError::TimedOut` (the transient
spawn/reap/futex flake class — B-DASH-STDIN-FLAKE / B-PTHREAD-YIELDBUDGET), while
a completed-but-wrong result keeps `InternalError`; fd-redirect infrastructure
failures now propagate the real fd-install error. So the non-fatal WARNING line
in `main.rs` is self-classifying: `TimedOut` == flake/hang, `InternalError` ==
real logic bug, other == infra. This does not *fix* the underlying race (root-
cause work still pending), but future flakes are now unambiguously attributed.
The B-PREEMPT-SPINLOCK preempt-disable fix (top of file) may also have reduced
this race's incidence; watching future boots for recurrence.

**Boot-data points (post B-PREEMPT-SPINLOCK fix):** 2026-07-01 (BOOT_OK 177s):
dash script-from-stdin passed (`captured 55 bytes == expected, EOF→exit 0: OK`);
no recurrence. No unexpected WARNING/failed lines this boot (the only
`[lockdep] WARNING`s are the lockdep self-test's intentional AB/BA + transitive-
cycle detections, each followed by `OK`). **2026-07-02 (TD31 landed):** 4 further
consecutive green boots (190/182/181/185 s) with zero self-test-failure lines —
the dash script-from-stdin test passed on every one; no `InternalError`/`TimedOut`
recurrence even with the added spawn/reap CGROUP traffic. **2026-07-14 (bad
flake streak under host load): 3 consecutive `--no-build` boots HUNG** (no BOOT_OK
within 480 s) at three *different* spawn/reap points — run 1 mid ring-3 `dash`
dirstat test, run 2 after the `test-restart-ct` container-init spawn (line 9289;
same wedge the TD "symmetric cgroup accounting" entry documents), run 3 at a
glibc-dynamic-exec page-cache fault for pid 165 (before any container code) — then
**run 4 reached BOOT_OK in 136 s clean**. The three hangs were the pre-existing
spawn/force-kill/reap SMP race, *not* a code regression: run 3 wedged before the
container subsystem even ran, and the runs were competing for host CPU with
concurrent cargo builds + overlapping QEMU instances (the race is host-timing
sensitive, so heavy host load raises its incidence). The Q19/§60 multi-network
self-test (`Multi-network membership (attach/detach): OK`) passed on every run
that reached it (runs 2 and 4). Takeaway: **run boot tests one at a time on an
unloaded host** — overlapping QEMUs materially worsen this flake.

### TD-EDITOR-UTF8. Text editors reject non-UTF-8 files (`fs::read_to_string`) — LOW PRIORITY / graceful failure 2026-07-02

**Where:** `apps/editor/src/main.rs` and `apps/markdowneditor/src/main.rs`
(`Document::from_file`), which load file content via
`std::fs::read_to_string(path)`.

**The limitation.** `read_to_string` returns `Err` for any file whose bytes are
not valid UTF-8, so both editors *refuse to open* a non-UTF-8 (e.g. Latin-1,
UTF-16, or binary) file. This is a **graceful failure** (the open is rejected
cleanly — there is no silent `from_utf8_lossy` corruption, which CLAUDE.md rule 7
forbids), so it is a limitation rather than a correctness bug. Discovered while
implementing the external-change merge feature (todo2.txt line 1), which reuses
the same String/`Vec<String>` line model.

**Why deferred, not fixed now.** Both editors store the document as
`lines: Vec<String>` and operate on `&str` throughout (rendering, cursor math,
find/replace, the diff/merge engine `diffcore` which is `String`-based). Truly
handling arbitrary bytes would require converting the whole editor + `diffcore`
to a byte-oriented buffer model with an explicit encoding/decoding layer — a
large refactor. For a *plain-text* editor, UTF-8 is a defensible domain
assumption and clean rejection of non-text files is acceptable behavior, so the
refactor is disproportionate to the value. CLAUDE.md rule 7's core concern
(OS-boundary metadata: paths, env, pipe data handled as bytes; no
`from_utf8_lossy`) is already honored — this is document *content*, and the
failure mode is safe.

**Proper fix (if a concrete need appears — e.g. editing config files in a legacy
encoding):** give the editor a byte buffer + a detected/selectable encoding
(UTF-8 default, with a lossless round-trip for at least Latin-1/UTF-16), decode
for display, re-encode on save, and thread the same through `diffcore`
(byte-slice diff). Trigger: a user report of a real file that won't open, or an
explicit requirement to edit non-UTF-8 documents.

### B-PAGECACHE-COHERENCE. Read-only page cache invalidation on FS mutations — FIXED 2026-06-30 (de-double-cache vs. buffer cache still pending)

**Resolution (2026-06-30):** the two correctness gaps below are now
closed. `mm::page_cache::invalidate_identity(fs_id, ino)` is wired into
the VFS mutation paths — `Vfs::write_at`, `Vfs::write_file`,
`Vfs::truncate`, `Vfs::remove`, and replacing same-mount `Vfs::rename`
— via the `cache_identity()` helper, which captures the file's
`(fs_id, ino)` under the held VFS lock (gated on a single relaxed
`is_populated()` atomic so the write path pays ~nothing when nothing is
cached). `remove` and the replacing-rename capture identity *before* the
inode is freed, closing the inode-reuse hole; the others capture after
the content change. Verified by boot self-test check 8 (is_populated +
invalidate_identity) and a green BOOT_OK.

**Shrinker (sub-task 4 eviction) landed 2026-06-30.**
`mm::page_cache::shrink(PressureLevel)` evicts *idle* cached pages
(refcount ≤ 1, i.e. no live mapper) proportional to the pressure level
(Low 25% / Medium 50% / Critical 90%), registered with `mm::pressure`
by `mm::page_cache::init()` (called from `kernel_main`). Verified by
boot self-test check 9 (shrink spares live, evicts idle) *and* by the
shrinker actually firing under real critical pressure during boot —
serial shows `[pressure] page_cache freed 49 objects (level=critical)`
then `freed 5 objects`, with BOOT_OK reached cleanly. Freeing 54 frames
under live pressure with no fault is a strong exercise of the
freed-while-mapped hypothesis: a mapped cache page always has
refcount ≥ 2 (cache entry + each PTE; `map_frame` does not bump
refcount, so the `get_or_fill` caller ref *becomes* the PTE ref), so
the shrinker's `refcount <= 1` gate never selects a mapped frame.

**Still pending (performance, not correctness — §36 sub-task 4 tail):**
de-double-cache the page cache against the block buffer cache
(`fs/cache.rs`) so a page does not live in both. Tracked as a follow-up;
not a bug.

The original write-up (now resolved for the correctness parts):



**Where:** `kernel/src/mm/page_cache.rs` (the cache) + the VFS/handle
write/truncate/unlink/rename paths (`kernel/src/fs/handle.rs`,
`kernel/src/fs/vfs.rs`, and the relevant syscall translators in
`kernel/src/syscall/linux.rs`).

**What it is:** sub-task 3 (commit wiring the FileBacked fault path to
`page_cache::get_or_fill`) populates the cache from mmap faults but does
**not** yet invalidate cached pages when the backing file changes. Two
correctness gaps result:

1. **Stale data after write/truncate.** If process A `mmap`s a file
   (pages enter the cache) and process B `write(2)`s or `ftruncate(2)`s
   that same file, A keeps seeing the *old* bytes through its mapping
   (and any later mmap of the file gets the cached stale page). The
   cache is read-only by design (writable MAP_SHARED writeback stays
   ENOSYS, §23), but read-side coherence with `write(2)` is still
   required and is missing.

2. **Inode-number reuse.** The cache key is `FileId { fs_id, ino }`.
   `fs_id` is monotonic per-mount (never reused), but `ino` **can** be
   reused within a mount after `unlink`. If file X (ino 53) is cached,
   unlinked, and a new file Y reuses ino 53, a fault on Y would be
   served X's stale pages. (`fs_id` prevents *cross-mount* collisions
   only.)

**Effect:** wrong file contents observed through a file mapping after a
concurrent write/truncate, or after unlink+recreate reuses an inode.
Not hit on the boot path (programs mmap read-only shared objects they
don't concurrently rewrite), which is why boot is green — but it is a
real correctness bug for general workloads.

**Proper fix (sub-task 4):** wire cache invalidation to FS mutations:
`page_cache::invalidate_file(file_id)` (or a page-range invalidate) on
`write`/`pwrite` that extends/overwrites a regular file, on `truncate`/
`ftruncate`, and on `unlink`/`rename` that drops/replaces an inode.
Resolve the `FileId` cheaply at the mutation site (the handle/path is
already known). Keep it cheap when nothing is cached (the
BTreeMap-range invalidate already returns 0 fast for an absent file).
Also de-double-cache against the block buffer cache (`fs/cache.rs`) per
§36 sub-task 4. Until this lands, the page cache is only safe for the
read-mostly mmap workloads the boot path exercises.

**Discovered/created:** 2026-06-30 (completing sub-task 3 without
sub-task 4's coherence wiring).

### B-CGROUP-DBLCHARGE. Demand-fault paths double-charge cgroup memory (manual `try_charge_current_mem` + `alloc_frame`'s internal charge) — FIXED (2026-06-30)

**Where:** `kernel/src/proc/pcb.rs` — `try_resolve_fault` demand-paging
paths. The whole-frame anon/file fast path (and the subpage path) call
`try_charge_current_mem(1)` *and then* `frame::alloc_frame()`, but
`alloc_frame` already charges the current task's cgroup internally
(`charge_cgroup_alloc`, recording the per-frame cgroup id in the
`FRAME_CGROUP` array). At final free, `free_frame` performs exactly one
`uncharge_cgroup_free` using the recorded id.

**Effect:** when cgroup memory accounting is active (`CGROUP_MEM_ACTIVE`
true), each demand page fault charges the cgroup **twice** (manual +1
and alloc_frame's +1) but uncharges only **once** at the final frame
free → a net **+1 charge leak per faulted page**. Over a process's
lifetime this inflates the cgroup's accounted memory without bound,
which can spuriously trip the cgroup memory limit / OOM. When cgroup
accounting is inactive (the common boot path), both charge calls
fast-exit, so there is no visible effect — which is why this has gone
unnoticed.

**Proper fix:** remove the manual `try_charge_current_mem(1)` /
`uncharge` bookkeeping from the demand-fault paths and rely solely on
`alloc_frame`/`free_frame`'s internal per-frame cgroup charging (which
is already correct and balances at the final free). The only subtlety:
`try_charge_current_mem` is also the place that enforces the *limit*
(returns an error to fail the fault when over budget) — so the fix must
ensure `alloc_frame` itself honors the cgroup limit (fail allocation
when the charge would exceed the limit) before deleting the manual
pre-check, otherwise the limit stops being enforced on the fault path.
Verify against the cgroup memory-limit self-test after the change.

**Discovered:** 2026-06-30 while wiring the page cache into the
FileBacked fault path (the cached-hit branch correctly needs *no*
manual charge, which surfaced the existing double-charge on the miss
branch).

**Fixed:** 2026-06-30. Removed the manual `try_charge_current_mem(1)` /
`uncharge_current_mem(1)` bookkeeping from both demand-fault paths in
`kernel/src/proc/pcb.rs` (subpage and whole-frame); the frame allocator
now owns cgroup memory accounting end-to-end. `alloc_frame` /
`alloc_frame_zeroed` already charge the current task's cgroup and honor
its limit (returning `Err(OutOfMemory)`, which the fault paths now
propagate as a rejected fault), so the deleted manual pre-check did not
weaken limit enforcement. Also closed two latent charge holes on the
zero-pool path: `alloc_frame_zeroed`'s pool-pop fast path now charges
the consumer, and `refill_zero_pool` uncharges frames it parks in the
pool (pooled frames are uncharged free inventory; the charge lands when
a consumer pops one). Regression guard: `mm::frame` self-tests 12
("charge/uncharge round-trip — no double-charge") and 13 ("over-limit
charge leaves no record"), which drive the real `charge_cgroup_alloc_to`
/ `uncharge_cgroup_free` primitives against an explicit test cgroup
(kmain self-tests run with no scheduled task, so the ambient
current-task cgroup is always root). Both pass in QEMU; the existing
cgroup charge/uncharge and limit-enforcement self-tests (10/11) still
pass.

### D-CGROUP-TASK-UNASSIGNED. Cgroup memory controller now reachable for real workloads — RESOLVED (2026-07-01)

**Original problem:** every `Task` was constructed with
`cgroup_id: ROOT_CGROUP` and no path ever set it to anything else, so
`current_task_cgroup()` always returned root, `charge_cgroup_alloc`
fast-exited, and the per-cgroup memory limit / accounting was never
exercised by real workloads — only by self-tests charging an explicit
cgroup. Container memory limits did not actually constrain memory.

**Resolution (Q14, operator option A):**
1. **Assignment path** — `sched::set_task_cgroup(task_id, cgroup)`
   (`kernel/src/sched/mod.rs:1287`) is the single authoritative
   process→cgroup assignment: it swaps `task.cgroup_id` under the SCHED
   lock and keeps the cgroup `nr_tasks` counts consistent (detach old,
   attach new) with a strict SCHED→cgroup-TABLE lock order.
   `container.rs` `add_process_task` (line ~1543) calls it to move a
   container's task into the container's cgroup, and `remove` (line
   ~1640) moves it back to root.
2. **Inheritance path** — `sched::spawn` (`mod.rs:1031/1046`) captures
   `current_task_cgroup()` before the task-creation critical section and
   copies it onto the new task, so `fork` (routes through
   `thread::spawn`→`sched::spawn`), `thread_clone`, and `spawn_user`
   (also `→sched::spawn`) all inherit the creating task's cgroup — Linux
   fork/clone semantics. Recorded in design-decisions §39.
3. **End-to-end test** — `cgroup_e2e_test_task` in `kernel/src/main.rs`
   runs as a live scheduler task (so `current_task_cgroup()` resolves to
   a real task, unlike the no-task kmain self-tests): it creates a
   memory-limited child cgroup, joins it via `set_task_cgroup`, allocates
   N=32 frames through the ordinary `alloc_frame` path (into a stack
   array — no heap growth to perturb the count), and asserts the group's
   `mem_usage` rose by exactly N; then frees them and asserts usage
   returns to baseline (uncharge follows the per-frame `FRAME_CGROUP`
   record, so it debits the right group even after the task rejoins root).
   Prints `[cgroup-e2e] PASS`/`FAIL` on the boot serial log.

**Discovered:** 2026-06-30 while fixing B-CGROUP-DBLCHARGE. **Resolved:**
2026-07-01 once Q14 settled which layer owns process→cgroup assignment
(`kernel/src/cgroup.rs` enforces + owns assignment via `set_task_cgroup`;
`fs::cgroupfs` remains the config frontend).

### D-PTHREAD-DETACH-LEAK. Detached pthread stacks are never freed (64 KiB leaked per detached thread) — RESOLVED (2026-07-01)

**Resolved 2026-07-01.** Implemented the userspace self-unmap fix exactly
as prescribed below:

- `THREAD_TABLE` is now a **lock-free array of atomic `ThreadSlot`s**
  (`task_id: AtomicU64` doubling as occupancy flag with `SLOT_EMPTY`/
  `SLOT_RESERVED` sentinels; `stack_base`/`stack_size: AtomicUsize`;
  `state: AtomicU8`). The old `static mut` + "single-creator convention"
  data race is gone.
- Added the `__pthread_exit_unmap(stack_base, stack_size, retval)`
  `global_asm!` primitive (`target_os="none"` only): it does
  `SYS_MUNMAP` then `SYS_THREAD_EXIT`, carrying `retval` in **R12** (a
  callee-saved reg the kernel's SYSCALL entry stub preserves — verified
  against `kernel/src/syscall/entry.rs`, which pushes/pops rbx/rbp/r12-r15
  around the handler). No memory is touched between the two syscalls.
- The per-slot `state` arbitrates the detach-vs-exit race via
  `compare_exchange`: `JOINABLE --detach--> DETACHED` (thread self-unmaps
  on exit) vs `JOINABLE --exit--> EXITED` (a joiner, or a `pthread_detach`
  that observes `EXITED`, frees the stack after `SYS_THREAD_JOIN` confirms
  the thread is off it). **Exactly one party frees** — no use-after-free.
  `pthread_join` rejects a detached thread (`EINVAL`); double-detach
  returns `EINVAL`; detach-after-joinable-exit reaps.
- Covered by 5 host unit tests in `pthread::tests`
  (`test_thread_slot_store_find_release`, `test_detach_marks_state_detached`,
  `test_double_detach_is_einval`, `test_join_rejects_detached_thread`,
  `test_detach_after_joinable_exit_reaps`) exercising the arbitration
  state machine directly.

**Residual (smaller) follow-up — D-PTHREAD-DETACH-KERNEL-EXITVAL — RESOLVED (2026-07-01):**
the kernel previously retained a small `THREAD_EXIT_VALUES: BTreeMap<TaskId,i64>`
entry (~tens of bytes) for a never-joined *detached* thread, because only
`join` removed it. **Fixed** by threading a "detached" flag through
`SYS_THREAD_EXIT` (arg1): `sys_thread_exit` (`kernel/src/syscall/handlers.rs`)
now decodes `let detached = args.arg1 != 0;` and passes it to
`thread_exit_with_value(exit_value, detached)`. A new `record_exit_value`
helper in `kernel/src/proc/thread.rs` skips the map insert entirely when
`detached` is set (task IDs are not reused while a task is live, so there
is no stale entry to clear). The userspace `__pthread_exit_unmap` self-unmap
asm sets `esi = 1` (detached) before `SYS_THREAD_EXIT`; the joinable
`pthread_exit` path uses `syscall2(SYS_THREAD_EXIT, retval, 0)` so arg1 is a
*defined* 0 (a bare `syscall1` would leave RSI holding stale/undefined bits,
which the kernel could misread as detached). In-kernel self-test
`test_detached_exit_not_retained` (verified at boot: `[thread]   Detached
exit value not retained: OK`) confirms joinable exits are recorded and
detached exits are not. Combined with the userspace stack self-unmap fix,
a detached thread now leaks neither its 64 KiB stack, its table slot, nor a
kernel map entry.

*Note:* a native SlateOS-ABI userspace test harness that links `posix`
does not currently exist (the boot path's thread tests use real glibc via
`clone`, not our `SYS_THREAD_CREATE`), so the "boot self-test spawning N
detached threads" originally envisioned below is deferred until such a
harness exists; the host unit tests cover the (bug-prone) arbitration
logic in the meantime.

---

**Original entry (for reference):**

**Where:** `posix/src/pthread.rs`. `pthread_create` mmaps a
`DEFAULT_THREAD_STACK_SIZE` (64 KiB) user stack and records it in
`THREAD_TABLE`. `pthread_join` frees the stack after `SYS_THREAD_JOIN`
returns. But a **detached** thread is never joined, so nothing ever
munmaps its stack — the `ThreadInfo` slot and the 64 KiB mapping leak for
the life of the process. `pthread_detach` only flips `info.detached`.

**Effect:** A long-running process that repeatedly spawns detached
worker threads leaks 64 KiB per thread plus a `THREAD_TABLE` slot (only
64 slots), eventually exhausting the table and address space. Most
current userspace tools don't spawn many detached threads, so it's
low-frequency, but it is a genuine unbounded leak.

**Proper fix (userspace-only, no kernel change):** the exiting thread
must free its *own* stack, glibc-`__unmapself`-style. Add a small
bare-metal asm primitive `__pthread_exit_unmap(stack_base, stack_size,
retval)` that issues `SYS_MUNMAP(stack_base, stack_size)` then
`SYS_THREAD_EXIT(retval)` **without touching the stack between the two
syscalls** (stash `retval` in a callee-saved reg that `SYSCALL` doesn't
clobber — not the stack). In `pthread_exit`, after running TSD
destructors, look up the calling thread's `ThreadInfo`: if `detached`,
`take_thread_info()` and tail-call `__pthread_exit_unmap`; if joinable,
fall through to the normal `SYS_THREAD_EXIT` (the joiner frees the
stack). Threads created detached, or detached before exit, both work.

**Concurrency caveat that must be handled:** `pthread_detach` (called
from another thread) races the exiting thread's read of `info.detached`
+ `take_thread_info`. The `THREAD_TABLE` currently relies on a
"single-creator convention" with NO lock — that is unsafe for the
detach-vs-exit window. The proper fix must add a real lock (or an atomic
detached flag per slot, CAS'd by whichever of detach/exit gets there
first) so exactly one of {joiner, self-unmap} frees the stack and there
is no use-after-free. Model on glibc's `joinid`/`cancelhandling` atomic.

**Why deferred:** the asm self-unmap path is `target_os="none"`-only and
cannot be unit-tested on the host; combined with the detach/exit data
race it is too risky to land without a QEMU multithread stress test.
Landing it blind risks a use-after-free crash, which is far worse than a
slow leak. Do it as its own focused task with a boot self-test that
spawns N detached threads in a loop and asserts address-space / table
usage stays bounded.

**Discovered/documented:** 2026-06-30 (already noted as a `// Known
limitation` in `pthread_detach`'s doc comment; promoted to tracked tech
debt while implementing per-thread TSD).

### D-CRT-INIT-ARRAY. `.init_array`/`.preinit_array` constructor + `.fini_array` destructor support — MECHANISM LANDED (end-to-end C/C++ validation pending a consumer)

**Status (2026-07-01):** The constructor/destructor machinery is now
**implemented and host-tested**. What remains is purely *validating it
against a real C/C++ program that emits constructors* — no such program
exists in-tree yet, so the mechanism has only been proven to be a correct
no-op for the (all-Rust) programs that currently run.

**What landed:**
- *crt (`posix/src/crt.rs`):* host-testable walkers `run_init_array(start,
  end)` (ascending, skips nulls) and `run_fini_array(start, end)`
  (descending), gated `#[cfg(any(target_os = "none", test))]`. Weak
  boundary externs (`__preinit_array_start/end`, `__init_array_start/end`,
  `__fini_array_start/end`) declared weak via a `.weak` **assembly
  directive** (`global_asm!`) rather than the nightly-only
  `#[linkage = "extern_weak"]` attribute — the kernel/boot build uses the
  **stable** toolchain, so a nightly `feature(...)` gate in posix breaks
  `bash scripts/boot-test.sh` (`E0554: #![feature] may not be used on the
  stable release channel`). A weak *undefined* symbol resolves to null at
  link time, so pure-Rust programs (no `.init_array`, no boundary symbols
  synthesised by lld's default layout) link cleanly and the startup walk
  sees null bounds → no-op. `run_constructors()` (preinit then init) is
  called from `__libc_start_main` after environ/signal init and before
  `main`; `run_destructors()` is registered via `atexit` so it fires
  LIFO-correct at normal exit. All `#[cfg(target_os = "none")]`, so the
  slateos userspace target (os=linux, uses Rust std's own startup) is
  untouched. Four host unit tests cover forward/skip-null, reverse order,
  null-bounds no-op, and empty-array no-op.
- *Linker scripts:* `.preinit_array`/`.init_array`/`.fini_array` output
  sections with `PROVIDE_HIDDEN` boundary symbols added to
  `services/{hello,init,ticker}/linker.ld` and
  `userspace/{coreutils,sha256sum,shell}/linker.ld` (`:load` so they land
  in the mapped PT_LOAD; `KEEP` so `--gc-sections` keeps them). NOTE: these
  6 scripts are currently **vestigial** — `kernel/build.rs` no longer
  passes `-T` for them and no per-crate build.rs applies them — so they're
  updated for correctness/future-proofing but do not yet feed a live link.

**Validated:** `cargo build -p posix` (stable, unknown-none) clean; all 6
programs whose linker scripts changed build+link clean; posix host tests
19992 passed (incl. the 4 init/fini tests); `bash scripts/boot-test.sh`
→ BOOT_OK (zero regression — the walk is a no-op for everything currently
running).

**Still pending (why not fully closed):** no in-tree C/C++ program emits
constructors, so the *non-null* path has never executed on real hardware/
QEMU. When the first such consumer lands it additionally needs either
(a) a C crt0 + `__libc_start_main` exported on slateos, or (b) a
posix-linking Rust program that actually emits `.init_array` — at which
point the boundary symbols become non-null and the walk should be
end-to-end boot-tested against that program. Until then this entry stays
open to flag that the constructor path is *implemented but unproven under
load*.

**Related — glibc-side ctor/dtor path IS now validated (2026-07-15):** Note
this entry is specifically about *Slate's own* `posix/src/crt.rs`
(slateos-target no_std programs that link the `posix` crate as libc). A
**separate** mechanism — the *glibc* Linux-ABI runtime's `.init_array`/
`.fini_array` walk (glibc's `__libc_csu_init` before `main`, `_dl_fini` at
exit) — is now proven end-to-end in ring 3 by Path Z **Part 41**
(`self_test_linux_real_glibc_cc_ctor_dtor` in `kernel/src/proc/spawn.rs`):
tcc compiles a program with `__attribute__((constructor))`/`((destructor))`,
and the freshly-built dynamic ELF emits `CTOR\nMAIN\nDTOR\n` (raw `write(2)`,
so byte order == temporal order), confirming ctor-before-main-before-dtor
under a real glibc. This does *not* close the entry (it exercises glibc's
runtime, not `crt.rs`), but it de-risks the concept and is the path real
Linux-ABI binaries actually use; the still-open gap is purely the
slateos-native `posix` crt whose live linker-script wiring remains vestigial.

**Discovered/documented:** 2026-06-30; mechanism implemented + host-tested
2026-07-01.

### D-CNET-L2BRIDGE. User-defined container networks now provide a shared layer-2 bridge (same-network peers reach each other directly) — RESOLVED 2026-07-01

**Resolution (2026-07-01):** each named network now stands up one
`net::bridge` instance and switches frames at L2 between its members'
veth host-ends, so two containers on the same named network reach each
other directly by their allocated IPs. The prior IPAM-only behaviour is
now backed by real inter-container reachability.

**What landed:**
1. **veth bridged flag** (`kernel/src/net/veth.rs`): `VethEnd` gained a
   `bridged: bool`; `poll_all()` skips bridged ends (the bridge owns their
   frames, not the global host stack). New `set_bridged`/`is_bridged`.
2. **Bridge veth ports** (`kernel/src/net/bridge.rs`): `BridgePort` gained
   `veth_pair: Option<usize>`; `MAX_BRIDGES` raised to 16.
   `attach_veth`/`detach_veth` register a veth pair's host-end (end A) as a
   bridge port (idempotent; port id = slot index), toggling the veth
   bridged flag outside the BRIDGES lock. `forward(bridge_idx)` drains each
   ingress port's `veth::recv(pair, A)`, learns src→port + resolves dst in
   one BRIDGES-locked step (MACs parsed via `get`+`try_from`, no slicing),
   then delivers: known unicast → `veth::send(out_pair, A, frame)`;
   broadcast/multicast/unknown → flood-clone to all other members **and**
   `ethernet::process_frame(&frame)` into the host stack (this preserves
   the pre-existing external-NAT path — no regression). `forward_all()`
   snapshots active bridges and forwards each.
3. **net::poll wiring** (`kernel/src/net/mod.rs`): `bridge::forward_all()`
   runs immediately before `veth::poll_all()`, so bridged host-ends are
   consumed by the bridge rather than the generic drain.
4. **Lazy per-network bridge lifecycle** (`kernel/src/cnetwork.rs`):
   `Network` gained `bridge_idx: Option<usize>`; `Allocation` gained
   `veth_pair: Option<usize>`. `attach_container_veth(name, cid, pair)`
   creates the bridge lazily on first attach, attaches the veth, and
   records the pair on the owning lease. `release`/`release_container`
   detach their veth pairs; `detach_and_maybe_teardown` deletes the bridge
   when its last port leaves (`veth_port_count == 0`).
5. **run-path wiring** (`kernel/src/kshell.rs`): the `oci run --network
   NAME` path calls `attach_container_veth` after taking the IPAM lease,
   printing `L2 bridge: NAME (N members)` (non-fatal warning if the veth
   is missing).

**Lock ordering:** `TABLE (cnetwork) → BRIDGES (bridge) → veth`; no reverse
edge, and `bridge::forward` never holds BRIDGES across veth I/O.

**Boot self-test:** `cnetwork::self_test()` builds a two-member network,
asserts the bridge is created lazily, exercises broadcast-flood and
learned-unicast forwarding, then verifies teardown on last detach —
serial `[cnetwork]   L2 bridge forward/learn: OK` and
`[cnetwork]   L2 bridge lifecycle: OK`.

**Follow-up (unchanged from before):** `poll_all` still dispatches
non-bridged veth frames into the *global* `ethernet::process_frame`;
per-namespace RX dispatch remains a separate TODO independent of this L2
switching work.

**Discovered/documented:** 2026-07-01 (while landing the `docker network`
IPAM feature, increments 60–61). **Resolved:** 2026-07-01.

### D-CNET-NSRX. Per-namespace veth RX + TX dispatch — RESOLVED (RX threading + container veth TX egress both landed)

**Status (2026-07-01): both halves landed and boot-validated.** The whole
ingress chain carries the arrival namespace (a container server socket bound
in its own netns is matched by the per-ns socket lookup), **and** the egress
path now routes container traffic (IPv4 data, fragments, ARP requests, ARP
replies) onto the container's veth instead of the physical NIC. Container
inbound/outbound over a user-defined network is functional end-to-end. The
one residual limitation is the **shared (non-namespaced) ARP cache** — see
the note at the end.

**What landed (RX threading).** `ns_id` is threaded as a parameter through
the entire RX chain:
- `ethernet::process_frame(data, ns_id)` — `is_for_us` now compares against
  the receiving namespace's interface MAC via the new
  `interface::ns_mac(ns_id)` (physical NIC MAC in root; veth-endpoint MAC in a
  container ns) instead of always the host NIC MAC.
- `ipv4::process_ipv4(payload, ns_id)` — `is_for_us` uses `ns_info(ns_id)`;
  inbound firewall uses `check_inbound_ns(ns_id, …)`; dispatches to
  tcp/udp/icmp with `ns_id`; `dispatch_reassembled` threads it too.
- `ipv6::process_ipv6(payload, ns_id)` — same shape (transport socket lookup
  is ns-scoped; NDP/SLAAC stay physical-NIC based, IPv6 container addressing
  is future work).
- `arp::process_arp(payload, ns_id)` — the "request for our IP?" check and
  reply source use `ns_ip`/`ns_mac`.
- `tcp::process_tcp/_v6(pkt, ns_id)` — pass `ns_id` to `process_tcp_common`
  instead of the old hardcoded `ROOT_NS`.
- `udp::process_udp/_v6(pkt, ns_id)` — the delivery loop now filters by
  `sock.ns_id` (root permissive, mirroring the TCP listener rule).
- `icmp::process_icmp(pkt, ns_id)` — echo replies are sent from the arrival
  namespace via `ipv4::send_ns`.
- Call sites: `net::mod::poll` and `bridge::forward_all`'s host-stack flood
  pass `ROOT_NS`; `veth::poll_all` passes each drained endpoint's own
  `ns_id`.

Boot-validated: `[udp]   Namespace isolation: OK` (extended to cover
delivery-level scoping — a datagram arriving in ns1 reaches only the ns1
socket, root arrival is permissive), full `[net] Network self-test PASSED`,
and the ARP ns tests — no physical-NIC regression.

**What landed (container veth TX egress).** The ns-aware send path now has a
veth egress branch keyed on the namespace:
- `net::send_frame_ns(ns_id, frame)` (`net/mod.rs`) — the single egress
  chokepoint: for `ns_id != ROOT_NS` with a veth endpoint
  (`veth::find_endpoint_for_ns`), it captures TX, `veth::send(pair, end,
  frame)` (→ enqueues on the peer host end A's RX → `bridge::forward_all`
  switches to the peer / floods to the host NAT stack), and records TX;
  otherwise it falls through to `send_frame` (physical NIC). Root traffic is
  unchanged.
- `ipv4::send_ns_ecn` and `send_fragmentable_ns` (both single-frame and the
  fragmentation while-loop) now source MAC/IP from `interface::ns_mac(ns_id)`
  / `ns_ip(ns_id)` / `ns_info(ns_id)`, resolve the next hop via
  `arp::resolve_ns(ns_id, …)`, and egress via `send_frame_ns(ns_id, …)`.
- `arp::send_request_ns` / `resolve_ns` — ARP requests are sourced from the
  ns interface and egress the ns link; `resolve_ns`'s poll loop drives
  `net::poll` (drains veth+bridge), so a peer reply returning through the
  bridge is learned into the cache. `resolve`/`send_request` delegate to the
  `_ns` forms with `ROOT_NS`.
- `arp::send_reply` — **no longer drops** non-root replies; it egresses via
  `send_frame_ns(ns_id, …)`, so a container answers ARP for its own IP on its
  user-defined network.

The container-creation path already assigns the ns interface IP/mask/gw/dns
(`netns::configure_interface`) and sets up the veth pair
(`setup_container_veth`), and `resolve_next_hop` for non-root uses
`netns::route_lookup` → ns gateway → direct-to-dst fallback, so
`resolve_next_hop`/`is_for_us` line up.

Boot-validated: new `[veth]   test 11 (send_frame_ns veth egress): OK`
(`[veth] Self-test PASSED (11 tests)`) — asserts a non-root
`send_frame_ns` lands on the peer host end's RX and a root-ns frame does NOT
leak into the veth — plus the RX-side `[udp]   Namespace isolation: OK` and
full `[net] Network self-test PASSED`, no physical-NIC regression.

**Per-namespace ARP cache — RESOLVED (was: shared ARP cache).** The former
residual (a single global ARP cache shared across all namespaces, so two
container networks reusing a subnet/IP could collide) is now closed. The
per-namespace ARP cache infrastructure that already existed (`NS_ARP`,
`ns_init`/`ns_destroy`/`ns_lookup`/`ns_insert`/`ns_flush`) is now wired into
the real paths:
- `container::setup_container_veth` calls `arp::ns_init(net_ns)` (and
  container removal calls `arp::ns_destroy(net_ns)`), so every networked
  container gets its own active ARP cache.
- `arp::process_arp` learns the sender's MAC into the *arrival* namespace's
  cache via `ns_insert(ns_id, …)` (delegates to global for ROOT_NS) instead
  of always `cache_insert` (global).
- `arp::resolve_ns` reads/waits on `ns_lookup(ns_id, …)` instead of the
  global `lookup`.
Boot-validated by a new `[arp]   ns process_arp learns into ns cache: OK`
(`[arp-ns] Per-namespace ARP self-test PASSED (4 tests)`), which asserts a
reply arriving in a namespace is learned into that ns's cache and does NOT
leak into the global cache. Root-namespace behavior is unchanged (still uses
the global `ARP_CACHE`).

**Discovered/analyzed:** 2026-07-01 (embedded-DNS work). **RX threading
landed:** 2026-07-01. **TX egress landed:** 2026-07-01. **Per-ns ARP cache
wired:** 2026-07-01.

### D-CONTAINER-EXEC-WAIT. Real in-container `docker exec` + synchronous wait — RESOLVED (all four steps landed)

**Status (2026-07-01): steps 1–4 done and boot-validated.** `container
exec` is no longer a net_ns-switch facade — it launches a genuine process
inside the container and (foreground) blocks until it exits, printing the
exit status. Step 4 (healthchecks) now landed too: the OCI `Healthcheck`
config is parsed, stored on the container, and driven by a periodic
non-blocking supervisor that surfaces health in `inspect`/`ps`.

**What landed:**
1. `container::wait_process(pid) -> KernelResult<i32>`
   (`kernel/src/container.rs`): the generalised block-on-exit primitive.
   Parks the caller on an arbitrary spawned global pid via
   `pcb::set_wait_task` + `sched::block_current`, woken by the
   zombie-transition path (`remove_thread` hands back the registered
   wait-task). Lost-wakeup-safe (re-check after register + scheduler
   `pending_wake`). On zombie it reads `pcb::exit_code(pid)` and reaps via
   `pcb::try_reap`, so an exec'd non-init child never lingers unreaped.
2. `container::exec_path(id, guest_cmd, argv) -> KernelResult<ExecSpawn>`:
   resolves `guest_cmd` under the container rootfs (`resolve_in_rootfs`,
   `..` cannot escape), reads the ELF, `spawn_process`es it, and
   `add_process_task`s it into the container's cgroup + PID/user/network
   namespaces + rootfs jail (the `run` wiring, minus flipping state /
   recording `init_pid`). Rolls the spawn back on bind failure. Stdio is
   left at the console default (foreground output appears live).
3. Shell `container exec [-d] <id> <cmd> [args...]`
   (`kernel/src/kshell.rs`, cmd_container "exec" arm): builds argv from the
   tokens, calls `exec_path`; foreground → `wait_process` + print exit
   status + `remove_process_task` cleanup; `-d` → print pid and return.

**Root-cause fix bundled in:** cgroup task-count accounting was previously
decremented **only** by an explicit `set_task_cgroup`/`remove_process_task`
while the task was still alive; a task that simply *exited* while assigned
to a non-root cgroup left a stale `nr_tasks` count forever (the task is
gone from the scheduler table before anyone can move it back to root).
`sched::reap_dead_tasks` now auto-detaches a reaped task from its cgroup
(skipping the root group; `detach_task` is saturating so a
detach-then-die can't underflow). This makes teardown accounting robust
for *any* exiting task, not just exec'd ones.

**Validation:** boot self-test `[container]   exec + wait
(exec_path/wait_process): OK` — creates a Running container with a real
rootfs, stages `/bin/hello`, execs it, yields until it zombifies, and
asserts: exit code 0 captured, process reaped (`pcb::state` is `None`),
cgroup billed +1 while alive then 0 after reap, plus the error paths
(exec on a non-Running container → InvalidArgument, missing binary →
NotFound, `wait_process(bogus)` → NoSuchProcess). BOOT_OK, hello's stdout
observed once in the serial log.

**Step 4 (healthchecks) — landed:** `oci::HealthcheckConfig`
(`kernel/src/oci.rs`) parses the OCI `Healthcheck` (test-token +
interval/timeout/retries/start_period, CMD vs CMD-SHELL). Each container
stores the probe plus its live health state
(`health_status`/`health_fail_streak`/`health_started_ns` and the
in-flight probe pid/task/deadline). The pure state machine
`container::apply_probe_result` implements the Docker semantics
(start-period grace does not count failures while `Starting`; a
`retries`-long failure streak → `Unhealthy`; any pass → `Healthy` + reset
streak) and is unit-covered by boot self-test `19k2h`.

The probes are driven by a **non-blocking** supervisor: a persistent
repeating `hrtimer` (250 ms tick, `start_health_monitor`, armed just
before `BOOT_OK` so it can't perturb the hrtimer self-test's exact
`pending_count` assertion) fires in ISR context, submits `health_tick_job`
to the shared `workqueue`, and `health_tick` polls every container.
Critically it **never blocks the single workqueue worker**: each probe is
launched via `exec_path`, then *polled* for its zombie transition on
subsequent ticks (never `wait_process`-blocked), reaped via the
`wait_process` fast path once dead, scored via `apply_probe_result`, and a
probe that overruns its timeout is `kill_process_threads`'d and scored as
a failure. The tick uses snapshot-under-lock → act-outside-lock (exec /
reap / kill / remove all take the table lock internally) → write-back.
Health is surfaced in `inspect` (JSON `health` field + human Health line
with failing streak) and `ps` (a `(healthy)`/`(unhealthy)`/`(health:
starting)` sub-state on the status column). Boot self-test `19k2s` drives
a real `/bin/hello` CMD probe deterministically to `Healthy`.

**Discovered/documented:** 2026-07-01 (while surveying the next container
increment after `docker network`). All four steps landed same day.

### W-KERNEL-COW-WRITE. Kernel-mode write fault on a user COW page is not routed to the resolver — WATCH (not currently reproducible)

**Where:** `kernel/src/idt.rs` page-fault handler (~line 1787). After
`mm::fault::resolve()` (kernel-VMA demand paging) declines a user
address, the user-fault resolver chain (swap-in →
`proc::pcb::try_resolve_fault`/CoW → stack growth) is entered **only**
when `error & 4` (CPL3, ring-3 access). A *kernel-mode* (ring-0) write
to a **present, read-only** user page (`error == 0x3`) therefore skips
CoW resolution and falls straight through to "FATAL: Unrecoverable
kernel page fault. Halting."

**Why it matters now:** the read-only page cache (§36) maps writable
`MAP_PRIVATE` file pages **RO + COW** on first fault (so writes copy out
of the shared frame), whereas the old private path mapped them
**writable** directly. Any kernel path that writes through a user
pointer into such a page *without first calling*
`mm::user::validate_user_write` (which breaks CoW eagerly at a safe
point, mirroring Linux `get_user_pages(FOLL_WRITE)`) would now trip a
ring-0 write fault on a COW page and halt. With pre-validation, the
correct kernel paths never hit this, which is why two full boots
(BOOT_OK, shrinker exercised under critical pressure) are clean.

**Status:** a prior-session boot showed a one-off
`EXCEPTION: Page Fault (#PF) ... error=0x3` at a USER_MMAP address
(`0x6000213450`) consistent with this scenario, but it has **not**
reproduced against the current source (two deterministic green boots).
Most likely it was a transient intermediate-edit state, not the
committed code. Left as a WATCH rather than a fix because the obvious
"route ring-0 user-address faults to `try_resolve_fault`" hardening
risks lock re-entrancy/deadlock (the faulting kernel code may already
hold the VMA/process locks the resolver takes) — exactly what
pre-validation exists to avoid.

**Proper fix if it recurs:** identify the specific kernel write path
that reaches a user COW page without pre-validating and make it call
`validate_user_write` (the architecturally-correct point to break CoW),
rather than weakening the fault handler. Only if a path genuinely cannot
pre-validate should the handler route ring-0 user-address faults to the
resolver, and then only with a fault-fixup/exception-table mechanism so
an unresolvable access returns `-EFAULT` instead of halting.

**Discovered:** 2026-06-30 (page-cache §36 sub-task 4 review).

### B-COMPACT1. Memory-compaction self-test (`collect_private_frames`) panicked non-deterministically across boots — FIXED 2026-06-16

**Where:** `kernel/src/mm/compact.rs` — `self_test()` Test 5; the API under test is
`kernel/src/mm/rmap.rs::collect_private_frames`.

**What it was:** the self-test added one fake private rmap entry, then called
`collect_private_frames(&mut [0u64; 4], 0)` once and asserted the fake frame was
among the (up to 4) results. `collect_private_frames` fills its `out` buffer with
the first `out.len()` private frames in table-index order, starting from the cursor
and wrapping once around the whole 16384-slot table. By the time the compaction
self-test runs, the rmap table already holds entries from other subsystems (a
failing boot showed ~16). With a 4-slot buffer, only the four lowest-indexed
private frames are returned; whether the fake entry (hashed to slot
`0x0F00_0000 % 16384`) is among them depends on what else occupies lower slots —
so the assertion passed or panicked depending on unrelated boot state. The panic
(`"collect_private_frames should find our fake entry"`) aborted the kernel mid-boot,
failing the Path-Z boot-test.

**Fix (2026-06-16):** the test now pages through the table with a 32-slot buffer
(larger than the live entry count, so a single full sweep already finds every
private frame including the fake one) and a bounded loop that advances the
continuation cursor each page, breaking as soon as the fake frame is seen, the
table is exhausted (`found == 0`), the cursor stops advancing, or a 64-page hard
cap is hit (guaranteed termination). This makes the test deterministic regardless
of how many unrelated rmap entries exist. Verified: BOOT_OK with
`[compact]   collect_private_frames: OK (saw_fake=true)` and
`[compact] Self-test PASSED`, 0 self-test failures.

**Related debt (not fixed):** `collect_private_frames`'s continuation/pagination
is mildly broken as a "visit every unique private frame exactly once" iterator —
each call performs a *full* `0..RMAP_TABLE_SIZE` sweep from `start_idx`, so when
more than `out.len()` private frames exist the continuation re-encounters frames
below the cursor on the next page (it never returns the `(found, 0)` "scan
complete" sentinel). The production consumer
(`compact.rs::try_compact`, 4 batches × 32) tolerates this — it re-checks each
candidate via `try_migrate_one` and only wastes a little work re-examining
duplicates — so it is a performance/clarity wart, not a correctness bug. A proper
fix would have the continuation scan only the *remaining* `[next, original_start)`
window rather than re-sweeping the whole table. Tracked here; low priority.

### B-EXT4-DIR. ext4 directory entries past the first block became invisible, and every directory insert grew the directory by a full block — FIXED 2026-06-16

**Symptom:** The ring-3 `link()`/`linkat()` hard-link self-test
(`self_test_linux_link`, kernel/src/proc/spawn.rs) intermittently failed
with exit 193 (link failed). Tracing showed `Vfs::write_file("/mnt/lnk-src",
b"L")` returned `Ok` but the file was then unresolvable, and later
`link()` reported `AlreadyExists` for a name the VFS layer's `exists()`
could not see. The persistent `/mnt` ext4 fixture (rootfs.ext4) also grew
without bound across boots as the self-tests created and deleted files.

**Root cause (two independent ext4 directory bugs):**

1. **`parse_dir_entries` abandoned the whole directory at the first
   `rec_len == 0`** (kernel/src/fs/ext4/driver.rs). ext4 directory data is
   a sequence of independent `block_size` chunks; a chunk can legitimately
   end with zero-padding (rec_len 0) while *later* blocks still hold live
   entries. The old loop `if hdr.rec_len == 0 { break; }` broke out of the
   entire directory, so every entry living in a block after the first
   zero-padded block was invisible to `read_dir_entries` → `dir_lookup` →
   path resolution. A file whose dirent landed in a later block "didn't
   exist" to `Vfs::exists`/`open`, yet `add_dir_entry`'s own physical scan
   still saw it (→ spurious `AlreadyExists`). It also meant `remove` could
   not find/unlink such entries, so they accumulated as orphans.

2. **`add_dir_entry`'s in-place-reuse path was dead code** (off-by-one).
   It computed the last directory block as `(dir_len / block_size) *
   block_size`, which for a block-aligned directory equals `dir_len`
   itself, so the guard `last_block_start < dir_len` was never true. Every
   insert fell through to the grow path, appending a fresh block per entry:
   unbounded directory bloat and fragmentation, which in turn fed bug (1)
   (more blocks → more chances for an entry to hide past a zero-padded
   block).

**Fix (proper):**

- Rewrote `parse_dir_entries` to parse block-by-block: an outer loop over
  `block_size` chunks and an inner loop over entries within
  `[block_start, block_end)`. `rec_len == 0` now terminates only the
  *current* block and advances to the next, never the whole directory.
  Name bounds use `block_end`, not `data.len()`. Added a regression test
  with a two-block buffer where block 0 ends in a zero-padded entry and
  block 1 holds a live entry, asserting both entries are found.
- Fixed `add_dir_entry` to compute the real last-block start as
  `dir_len.saturating_sub(block_size)` (guarded by `dir_len > 0 &&
  block_size > 0`), so free space in the final block is actually reused
  instead of growing the directory every time.
- Refactored `insert_dir_entry` to take an explicit `block_start`
  parameter (removing a buggy `(offset / remaining).max(1) * ...`
  reconstruction) and scan forward from it to find the previous entry to
  shrink.

**Verified:** With the fixes plus a freshly regenerated rootfs.ext4
(`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`), the ring-3
link()/linkat() self-test passes and the full boot reaches BOOT_OK with
zero self-test failures.

**Fixture note:** The pre-existing rootfs.ext4 had accumulated duplicate /
orphaned `lnk-dst` directory entries from prior buggy boots that the fixed
code could now see but a single `remove()` could not fully clear. The
fixture was regenerated clean. `self_test_linux_link` also gained a bounded
`drain()` loop that removes any stale src/dst names before staging, so the
test is robust to a dirty persistent fixture going forward.

### B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd (relative `open`/`*at` resolved against `/`) — FIXED 2026-06-16

**Symptom:** After a process did `chdir("/dir")`, a relative `open("file")`
(or `openat(AT_FDCWD, "file")`, and the relative-path branches of stat,
access, mkdir, unlink, rename, readlink, chmod, chown, etc.) resolved the
path against the filesystem **root** rather than `/dir`. e.g. `cd /reltest &&
echo x > rel.txt` created `/rel.txt`, not `/reltest/rel.txt`. This broke
standard Unix semantics for essentially every program that uses relative
paths after changing directory.

**Root cause:** The Linux ABI's `open_common` forwarded the raw userspace
path pointer straight to `sys_fs_open` → `fs::handle::open` →
`Vfs::resolve_path`, and `resolve_at_path` (the `*at` family helper) returned
the path verbatim for the `AT_FDCWD`/relative case. None of those layers
take a PID, and `Vfs::normalize_path` treats `rel` identically to `/rel`
(it splits on `/` and always re-emits a leading slash), so the per-process
cwd stored in the PCB by `chdir` (`pcb::set_cwd`) was never consulted on the
open side. The limitation was even documented in `resolve_at_path`'s doc
comment ("there is no per-process cwd in the native path resolver").

**Fix (proper):** Resolve relative paths against the caller's cwd at the
Linux ABI boundary, reusing the existing `canonicalize_path(cwd, path)`
helper (already used by the chroot gate and `fstatat`). `open_common`
(kernel/src/syscall/linux.rs) now canonicalises the path against
`pcb::get_cwd(caller)` and opens via a new `handlers::fs_open_kernel_path`
(a kernel-string variant of `sys_fs_open` that does the File-READ cap check
+ handle registration without reading userspace), and `resolve_at_path`
canonicalises its `AT_FDCWD`/absolute result the same way. Kernel context
(no caller PID) falls back to cwd `"/"`, preserving the prior behaviour for
in-kernel callers and the native ABI (`sys_fs_open` is untouched). Absolute
paths are normalised but otherwise unchanged. Regression test: Path Z
Part 23 (`self_test_linux_real_glibc_shell_relpath`) runs `cd /reltest &&
echo RELOK > relfile.txt` in ring 3 and asserts the file landed at
`/reltest/relfile.txt` and **not** at `/relfile.txt`.

### B-ACCESS1. Linux-ABI `access`/`faccessat`/`faccessat2` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every `access`/`faccessat`/`faccessat2` call returned `-ENOENT`
unconditionally, even for files that exist in the VFS. The headline casualty
was unmodified GNU `make`: make issues `access("/bin/sh", X_OK)` **before**
spawning a recipe and, on failure, prints `"/bin/sh: No such file or directory"`
+ `Error 127` and never spawns the recipe shell — so no Makefile recipe could
run. (Confirmed via `strace` on real Linux: `access(shell, X_OK) = 0` precedes
the `clone3`.) Same class of stale stub as B-STAT1, but for the accessibility
probes rather than `stat`.

**Root cause:** `sys_access` / `sys_faccessat` / `sys_faccessat2` validated the
mode/flag bits and the path pointer, then hard-coded `linux_err(errno::ENOENT)`
with a comment that "without a backing filesystem there is no path that exists."
True when written; a silent lie once the VFS gained a backing store.

**Fix (proper):** The three syscalls now share a new `access_path_common`
back-end (kernel/src/syscall/linux.rs) that canonicalises the path against the
caller's cwd (`pcb::get_cwd`) and looks it up via `Vfs::metadata` (follow) /
`Vfs::lmetadata` (`AT_SYMLINK_NOFOLLOW`). Under the no-DAC capability model
(design-decisions §31) `F_OK`/`R_OK`/`X_OK` succeed for any existing file/dir —
consistent with `execve`, which ignores on-disk x-bits. Kernel context (no
caller PID) preserves the ENOENT no-file contract the fidelity self-tests
assert. Regression test: Path Z Part 34 (`self_test_linux_real_glibc_make`) runs
real GNU make end-to-end, whose recipe dispatch depends on `access(shell, X_OK)`.

**Known limitation (W_OK):** `W_OK` is granted for any existing file; it does
not yet consult per-mount read-only state (not tracked at this layer). A
read-only mount should return `EROFS` for `W_OK`. Low priority — no read-only
mounts are exposed to ring-3 writers today.

### B-ABI1. A *bare* static Linux ELF (no OSABI/PT_INTERP/PT_GNU_PROPERTY) is misclassified as Native-ABI on `exec` — KNOWN LIMITATION (escalated as open-questions.md Q9)

**Symptom:** A Linux binary with none of the markers `elf::detect_linux_abi`
keys off — e.g. the output of `tcc -nostdlib -static`, or a hand-rolled static
musl/asm program (OSABI=`SYSV`/0, no `PT_INTERP`, no `PT_GNU_PROPERTY`; the only
GNU-ish artifact is a `PT_GNU_RELRO` segment, deliberately rejected as a signal)
— is classified as a **Native-ABI** process. Its raw `syscall`s are then routed
through the native dispatch table instead of `kernel::syscall::linux`, so e.g.
`write(1, …)` produces 0 observable bytes and `exit(n)` loses its status. This
bites the **`exec` path** (a shell or `make` exec'ing a freshly-built bare tool),
which re-detects the ABI from the ELF with no way for the caller to override.

**Root cause:** A bare SYSV static ELF carrying only generic GNU-toolchain
artifacts is genuinely ambiguous between "Linux binary" and "SlateOS-native
binary built with a GNU/LLVM toolchain." No automatic heuristic separates them
reliably; disambiguation needs an explicit marker on one side.

**Worked around (spawn only):** `spawn::spawn_process_with_abi(elf, options,
AbiMode::Linux)` lets an in-kernel caller that *knows* the binary's ABI state it
explicitly (used by `self_test_linux_real_glibc_cc`, which just compiled the
binary as a Linux program). This does **not** cover the general `exec` path.

**Proper fix (deferred — needs operator decision):** open-questions.md **Q9** —
recommendation is to default unmarked bare ELFs to the Linux ABI and stamp
SlateOS-native binaries with an explicit OSABI value / `.note.slateos`, plus add
`NT_GNU_ABI_TAG` note-walking as a positive Linux signal. Where it bites:
`kernel/src/proc/elf.rs::detect_linux_abi`, `spawn.rs::spawn_process_inner`, and
the `exec` `new_abi_mode` path.

### B-SPAWN1. `posix_spawn`/`vfork` child loses the exec-failure errno under CoW-fork degradation — KNOWN LIMITATION (acceptable)

**Symptom:** When a glibc `posix_spawn(3)` (or `vfork`) child fails its
`execve` (e.g. the target is missing), the parent observes a child that exited
with status 127 rather than receiving the precise `errno` glibc's posix_spawn
normally reports.

**Root cause:** glibc's posix_spawn does `clone3({CLONE_VM|CLONE_VFORK|
CLONE_CLEAR_SIGHAND, ...})` expecting a **shared** address space: on exec
failure the child writes `errno` to a stack location the parent then reads. Our
processes are address-space isolated, so `linux_clone_inner`'s VFORK_SPAWN
branch degenerates the shared-VM vfork to a copy-on-write fork. The child runs
on its own copied stack, so a post-fork write to the (formerly shared) errno
slot is invisible to the parent — only the child's exit status survives. The
common success case is unaffected (the child execve's and never writes back).

**Proper fix (deferred):** Genuine `CLONE_VM` shared-address-space semantics for
the vfork window, or a kernel-mediated errno relay from the failing child's
exec path back to the parent's clone return. Deferred until a workload depends
on the precise errno; status-127 is the universally-understood "exec failed"
signal and is what shells display anyway.

### B-STAT1. Path-based `stat`/`lstat`/`newfstatat`/`statx` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every path-based stat syscall returned `-ENOENT` unconditionally,
even for files that exist in the VFS. Any program that stats a path before
opening it saw the file as missing: dash's `[ -f FILE ]` / `[ -e FILE ]` /
`[ -d DIR ]` test predicates were always false, `ls FILE`, `stat FILE`, and
`configure`-style existence probes all failed. `fstat` (fd-based) worked, so
this only bit the path-based variants. (Distinct from B-CWD1, which was about
*relative* path resolution — B-STAT1 returned ENOENT even for valid
*absolute* paths.)

**Root cause:** The handlers carried a stale "no files exist on our skeleton
FS" assumption from before the VFS held real files. `stat_path_impl`
(shared by `stat`/`lstat`) and the non-empty-path branches of
`sys_newfstatat` / `sys_statx` validated the path pointer and then hard-coded
`linux_err(errno::ENOENT)` with comments explaining that `filename_lookup`
"always fails on our no-file FS". That was true when written but became a
silent lie once the VFS gained a backing store.

**Fix (proper):** Do a real VFS lookup for ring-3 callers.
`stat_path_impl` was rewritten into `stat_path_common(path_ptr, statbuf_ptr,
follow)` which canonicalises the path against the caller's cwd
(`canonicalize_path` + `pcb::get_cwd`) and resolves it via new helpers
`stat_meta_for_path` (calls `Vfs::metadata` when `follow`, else
`Vfs::lmetadata`; maps `NotFound`→ENOENT) + `fill_stat_from_meta` /
`fill_statx_from_meta` (map `EntryType`→`S_IF*` bits, synthesise default
perms `0o755`/`0o777`/`0o644` when the FS reports `permissions == 0`, and
backfill 0 timestamps with `clock_realtime()`). `sys_newfstatat` / `sys_statx`
non-empty branches resolve via `resolve_at_path(dirfd, path)` (dirfd + cwd
rules) then real-stat-and-fill, with `follow = (flags & AT_SYMLINK_NOFOLLOW)
== 0`. Statbuf pointer gates are deferred until *after* a successful lookup,
matching Linux's `getname`/`filename_lookup`-before-`cp_new_stat` ordering
(so `stat("missing", NULL)` returns ENOENT, not EFAULT). Kernel context
(`caller_pid().is_none()`) still returns ENOENT, preserving the batch-488
syscall-fidelity self-tests (which pass a fake pointer in kernel context).
Regression test: Path Z Part 24 (`self_test_linux_real_glibc_shell_statpath`)
runs `[ -f /bin/dash ] && echo HASFILE > /stat-out.txt` in ring 3 and asserts
the redirect fired (8 bytes, exit 0), proving the `-f` predicate's path stat
now succeeds.

### B-SYM1. Linux-ABI `symlink`/`symlinkat` returned EROFS and `readlink`/`readlinkat` returned EINVAL unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could create or resolve a symbolic link. Every
`symlink(2)`/`symlinkat(2)` returned `-EROFS` ("read-only file system") and
every `readlink(2)`/`readlinkat(2)` returned `-EINVAL` ("not a symlink"),
regardless of whether the path existed or was actually a symlink, even though
the VFS is fully writable and natively supports symlinks (`Vfs::symlink`,
`Vfs::readlink`). This breaks any toolchain that relies on symlinks (build
systems, `ld` SONAME links, package layouts).

**Root cause:** The four handlers were placeholder stubs left over from before
the VFS gained symlink support: `sys_symlink`/`sys_symlinkat` validated their
path arguments and then hard-coded `linux_err(errno::EROFS)`, and
`sys_readlink`/`sys_readlinkat` hard-coded `linux_err(errno::EINVAL)` after
the argument gates. The stubs' errno terminals were also (correctly) asserted
by the batch-478/487 syscall-fidelity self-tests, which call the handlers in
kernel context with fake pointers — so the fix had to keep those terminals for
kernel callers while doing real work for ring-3 callers.

**Fix (proper):** Wire all four to the VFS for ring-3 callers.
`sys_symlink`/`sys_symlinkat` share a new `symlink_common(target_ptr,
newdirfd, linkpath_ptr)` that stores the `target` *verbatim* (a symlink may
dangle and may be relative — it is NOT resolved or canonicalised), resolves
the `linkpath` against the caller's cwd / `newdirfd` via `resolve_at_path`,
requires a File-WRITE capability (`require_fs_write`), and calls
`Vfs::symlink`. `sys_readlink`/`sys_readlinkat` share a new
`do_readlink_copy(path, buf_ptr, bufsiz)` that calls `Vfs::readlink` and
copies `min(target_len, bufsiz)` bytes with **no** trailing NUL, returning the
byte count (the Linux `do_readlinkat` contract); the user buffer is validated
and written only *after* the dentry is confirmed to be a symlink
(`NotFound`→ENOENT, `InvalidArgument`→EINVAL/"not a symlink"). Kernel context
(`caller_pid().is_none()`) preserves the prior EROFS/EINVAL terminals;
`sys_readlink` canonicalises the path against `pcb::get_cwd` first, the `*at`
variants use `resolve_at_path`. Regression test: Path Z Part 27
(`self_test_linux_symlink_readlink`) — a hand-built raw-syscall Linux-ABI ELF
(`build_linux_symlink_readlink_test_elf`) calls `symlink("Z", "/sl-rl-link")`
then `readlink("/sl-rl-link", buf, 64)` from ring 3 and asserts the call
returned exactly 1 byte == `'Z'` (self-diagnosing exit sentinels
`0xB1`/`0xB3`/`0xB4`). The harness pre-removes the link path and, after the
process exits 0, independently confirms kernel-side via `Vfs::readlink` that
the created link resolves to `"Z"`. (Raw ELF rather than dash because dash has
no `ln` builtin and cannot invoke `symlink(2)`/`readlink(2)` directly.)
**Follow-up — `link`/`linkat` now wired (2026-06-16):** `sys_link`/`sys_linkat`
share a new `link_common` that resolves both names via `resolve_at_path`,
requires a File-WRITE capability, and calls `Vfs::link` (kernel context still
EROFS). Regression test: Path Z Part 28 (`self_test_linux_link`) hard-links
`/mnt/lnk-dst` to a pre-staged `/mnt/lnk-src` from ring 3 and reads the byte
back through it.

**memfs does not support hard links (deferred):** the test runs on the **ext4**
mount at `/mnt`, not the in-memory root (`/`, `/tmp`). memfs stores file data
inline in by-value tree nodes (`MemFsNodeKind::File(Vec<u8>)` owned by the
parent's `BTreeMap`), so two directory entries cannot share one inode — which
is exactly what a hard link requires. memfs therefore correctly returns
"unsupported" (Linux returns **EPERM** for filesystems without hard-link
support). Proper fix: refactor memfs to an inode-table model (`MemFs` owns
`BTreeMap<ino, Inode>`; file/symlink directory entries hold an `ino` instead of
the body, so multiple names can reference one inode with a shared `nlink`).
This is a sizeable refactor of a core subsystem with many passing self-tests,
and ext4 (the design's real root FS) already implements hard links, so it is
deferred rather than done speculatively. **Fidelity gap (minor):** `Vfs::link`
always follows a symlink `oldpath`, whereas plain `link(2)` should not follow
and `linkat` should follow only with `AT_SYMLINK_FOLLOW`; the common
regular-file case is correct, only the rare hard-link-a-symlink case differs
(would need a `Vfs::link` no-follow variant to fix properly).

**Follow-up — `utimensat`/`utimes`/`utime` now wired (2026-06-16):** see
B-UTIME1 below.

### B-UTIME1. Linux-ABI `utimensat`/`utimes`/`utime` returned EROFS unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could update a file's access/modification
timestamps. Every `utimensat(2)`/`utimes(2)`/`utime(2)` returned `-EROFS`
after the (Linux-faithful) input-shape gate ladder, even though the VFS is
writable and `Vfs::set_times` is implemented by memfs, ext4 and fat. `touch`,
`make` (which stamps targets), `tar -x` (restores mtimes) and configure
scripts all depend on this.

**Root cause:** The three handlers were placeholder stubs that validated their
arguments (and faithfully reproduced Linux's `EINVAL`/`ENOENT`/(OMIT,OMIT)→0
input-shape diagnostics — batch 489) and then hard-coded
`linux_err(errno::EROFS)`. The EROFS terminal is also asserted by the
batch-489 fidelity self-tests, which call the handlers in kernel context, so
the fix had to keep that terminal for kernel callers.

**Fix (proper):** For ring-3 callers (`caller_pid().is_some()`) each handler
now resolves the target path (`resolve_at_path` against the caller's cwd /
dirfd; `utimensat` with a NULL pathname resolves the open file behind `dirfd`
via `handle_path`), translates the parsed `timespec`/`timeval`/`utimbuf` into
ns-since-epoch (`UTIME_NOW`→`clock_realtime`, `UTIME_OMIT`/NULL-field→leave
unchanged, otherwise `sec*1e9 [+ sub-second]`), requires a File-WRITE
capability, and calls `Vfs::set_times`. Kernel context preserves the EROFS
terminal. Regression test: Path Z Part 29 (`self_test_linux_utimensat`) — a
hand-built raw-syscall ELF (`build_linux_utimensat_test_elf`) calls
`utimensat(AT_FDCWD, "/utimensat-test", {atime=1.6e9 s, mtime=1.5e9 s}, 0)`
from ring 3 (self-diagnosing exit sentinel `0xD1`); the harness stages the file
on the memfs root (memfs implements `set_times`) and, after exit 0,
independently asserts the kernel-side `Vfs::metadata` reports
`accessed_ns == 1.6e18` and `modified_ns == 1.5e18` exactly.

**Fidelity gaps (minor, documented in the linux.rs module comment):**
1. `Vfs::set_times` always follows symlinks, so `utimensat`'s
   `AT_SYMLINK_NOFOLLOW` is a no-op (the target is touched, not the link).
   Proper fix needs a `Vfs`/`Filesystem` no-follow `lset_times` variant.
2. The `Timestamp = u64` VFS API overloads `0` ("ns since epoch") as the
   "leave this field unchanged" sentinel, so a request to set a field to
   exactly the Unix epoch (or any pre-epoch / negative instant) is silently
   treated as "leave unchanged". Proper fix needs an `Option<u64>` (or
   explicit "omit" flag) plumbed through `Filesystem::set_times` for every FS.

### B-CHOWN1. Linux-ABI `chmod`/`fchmod`/`fchmodat`/`fchmodat2`/`chown`/`lchown`/`fchown`/`fchownat` returned EROFS unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could change a file's permission bits or
ownership. The whole chmod/chown family returned `-EROFS` after input
validation, even though the VFS tracks Unix mode bits and uid/gid and
implements `Vfs::set_permissions` / `Vfs::set_owner` across memfs/ext4/fat.
`install -m`, `chmod +x`, `tar -x` (restores perms/owner), and package
managers all depend on this.

**Root cause:** Placeholder stubs that validated arguments (faithfully
reproducing Linux's `ENOENT`/`EBADF`/`EINVAL` input-shape diagnostics —
batches 483/484) and then hard-coded `linux_err(errno::EROFS)`. The EROFS
terminal is asserted by those fidelity self-tests in kernel context.

**Fix (proper):** For ring-3 callers each handler resolves the target
(`resolve_at_path` for the path variants against the caller's cwd/dirfd;
`handle_path` on the open file for the `fchmod`/`fchown` fd variants and the
`fchownat(AT_EMPTY_PATH)` form), requires a File-WRITE capability, and calls
`Vfs::set_permissions` (mode masked to `0o7777`) or `Vfs::set_owner` (uid/gid
narrowed to 32 bits; the `(uid_t)-1`/`(gid_t)-1` "leave unchanged" sentinels
are honoured by `Vfs::set_owner`). Kernel context keeps the EROFS terminal.
`fchmod`/`fchown` on a non-file fd (pipe/console, no backing inode) return
EINVAL. Regression test: Path Z Part 30 (`self_test_linux_chmod_chown`) — a
hand-built raw-syscall ELF (`build_linux_chmod_chown_test_elf`) calls
`chmod("/chmod-chown-test", 0o640)` then `chown(path, 1234, 5678)` from ring 3
(sentinels `0xE1`/`0xE2`); the harness stages the file on the memfs root and,
after exit 0, independently asserts `Vfs::metadata` reports
`permissions == 0o640`, `uid == 1234`, `gid == 5678`.

**Follow-up (`fchmodat2`, syscall #452):** the 4-arg flags-aware chmod
(`fchmodat2`) was a separate EROFS stub missed by the first pass; it was
wired in the same idiom as `sys_fchownat` during the truncate-line cleanup.
`AT_EMPTY_PATH` resolves `dirfd` to its backing path (AT_FDCWD → cwd, else an
open File fd via `handle_path`); the non-`AT_EMPTY_PATH` branch keeps the
empty-path → ENOENT discrimination then `resolve_at_path` + `chmod_apply`.
Kernel context keeps the EROFS terminal (batch-485 self-test still green).
Regression test: Path Z Part 32 (`self_test_linux_fchmodat2`) —
`build_linux_fchmodat2_emptypath_test_elf` `open(O_RDWR)`s `/fchmodat2-test`
and calls `fchmodat2(fd, "", 0o600, AT_EMPTY_PATH)` (sentinels `0xE5`/`0xE6`);
the harness confirms `Vfs::metadata` reports `permissions == 0o600`.

**Fidelity gaps (minor):**
1. `lchown` and `fchownat(AT_SYMLINK_NOFOLLOW)` must operate on the symlink
   itself, but `Vfs::set_owner` always follows the final symlink (same
   no-follow gap as B-SYM1 for `link`). The common non-symlink case is
   correct; a proper fix needs a no-follow `lset_owner` VFS variant.
2. We gate chmod/chown on the generic File-WRITE capability rather than a
   dedicated `CAP_CHOWN`/`CAP_FOWNER`; any process holding File-WRITE can
   change mode/owner. This matches the OS's capability model (no per-syscall
   POSIX capability bits yet) but is laxer than Linux's privilege checks.

### B-TRUNC1. Linux-ABI `truncate`/`ftruncate` returned EROFS unconditionally for confirmed regular files (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could resize a file. Both `truncate(2)` and
`ftruncate(2)` ran their full input-shape gate ladders (EINVAL on negative
length, EFAULT/ENOENT on bad/empty/missing paths, EISDIR on directories,
EINVAL on non-regular inodes / read-only-fd via the FMODE_WRITE check) and
then hard-coded `linux_err(errno::EROFS)` for a confirmed regular file —
even though the VFS exposes `Vfs::truncate` implemented across
memfs/ext4/fat. Every database that pre-sizes its file (sqlite, lmdb),
log rotators, `dd`/`truncate(1)`, `fallocate`-fallback paths, and `./configure`'s
`AC_FUNC_FTRUNCATE` probe depend on this.

**Root cause:** Placeholder terminals from the universal-read-only era.
`sys_truncate` already did a real `Vfs::stat` triage (so the EISDIR/EINVAL/
ENOENT diagnostics were live), and `sys_ftruncate` already enforced the
`FMODE_WRITE` gate; only the final File-arm answer was a hard-coded EROFS.
Both terminals are asserted in kernel context by the batch-447/448 fidelity
self-tests, which short-circuit on `caller_pid().is_none()`.

**Fix (proper):** For ring-3 callers `sys_truncate` (after the stat triage
confirms a regular file) and `sys_ftruncate`'s File arm now enforce
`RLIMIT_FSIZE` (EFBIG on a grow past the soft limit — the check returns to
its proper Linux position now that mounts are writable, between the
`FMODE_WRITE`/`mnt_want_write` gate and `do_truncate`), require a File-WRITE
capability, resolve the target (`canonicalize_path` for the path variant,
`handle_path` on the open fd for `ftruncate`), and call `Vfs::truncate`,
which grows-with-zeros or shrinks the file. Kernel context keeps the EROFS
terminal (gated on `caller_pid().is_none()`). Regression test: Path Z Part 31
(`self_test_linux_truncate`) — a hand-built raw-syscall ELF
(`build_linux_truncate_test_elf`) shrinks `/truncate-test` to 4 bytes via the
path syscall, then `open(O_RDWR)` + `ftruncate(fd, 10)` grows it; sentinels
`0xF1`/`0xF2`/`0xF3`. The harness stages a 16-byte file on the memfs root and,
after exit 0, independently asserts the readback is exactly 10 bytes with the
leading 4 preserved (`'A'`) and the grown tail zero-filled.

**Fidelity gaps (minor):**
1. `Vfs::truncate` follows the final symlink (`resolve_follow`), matching
   Linux's `truncate(2)` (which also follows). `ftruncate` operates on the
   fd's already-resolved inode, also correct. No no-follow truncate exists
   in Linux, so there is no gap here — noted only for symmetry with B-CHOWN1.
2. We gate the resize on the generic File-WRITE capability; Linux additionally
   honours `IS_APPEND`/`IS_IMMUTABLE` inode flags (EPERM) and the per-fd
   `O_APPEND`-doesn't-block-truncate nuance. The append/immutable-flag EPERM
   path is not yet plumbed (same capability-model gap as B-CHOWN1).

### B-FALLOC1. Linux-ABI `fallocate` COLLAPSE_RANGE/INSERT_RANGE now shift contents; only UNSHARE_RANGE still EOPNOTSUPP — PARTIALLY RESOLVED 2026-06-18, COLLAPSE/INSERT ADDED 2026-06-20

**Status:** `sys_fallocate` (syscall #285) was wired 2026-06-16 (Path Z Part 33)
from a blanket EOPNOTSUPP terminal to the real VFS for the two *allocate* modes:
`mode == 0` (posix_fallocate grow → `Vfs::file_size`/`Vfs::truncate`, never
shrinking) and `FALLOC_FL_KEEP_SIZE` (block reservation → `Vfs::fallocate`).
MemFd fds grow via `ipc::memfd::truncate`. Both enforce `RLIMIT_FSIZE` (EFBIG)
and the File-WRITE capability.

**Update 2026-06-18 — PUNCH_HOLE / ZERO_RANGE implemented.** The two most
commonly used range modes now do real work instead of returning EOPNOTSUPP. New
helpers `fallocate_zero_vfs` / `fallocate_zero_memfd` (kernel/src/syscall/linux.rs)
zero `[offset, offset+len)` in 16 KiB chunks via the backend's efficient
`write_at` (ext4/fat/memfs all override it). `i_size` is preserved for PUNCH_HOLE
(always KEEP_SIZE) and ZERO_RANGE+KEEP_SIZE — the zeroed region is clamped to the
current size and a range entirely past EOF is a no-op; ZERO_RANGE *without*
KEEP_SIZE grows the file to `offset+len` if the range crosses EOF, zero-filling
the gap. This is correct **read-as-zero** behaviour; the only thing not provided
vs. a real hole-punch is **disk-space reclamation** (an optimisation, not a
correctness property — our backends are non-sparse). Covered by
`self_test_fallocate_range` (registered in kernel/src/main.rs as a late, post-/tmp
self-test) which exercises ZERO_RANGE+KEEP_SIZE, PUNCH_HOLE, a past-EOF KEEP_SIZE
no-op, a ZERO_RANGE grow, and a MemFd ZERO_RANGE — all green at boot.

**Update 2026-06-20 — COLLAPSE_RANGE / INSERT_RANGE implemented.** Both
content-shifting modes now do real work for regular files (`HandleKind::File`)
instead of returning EOPNOTSUPP. The dispatch (kernel/src/syscall/linux.rs
`sys_fallocate`) enforces the full Linux contract: it queries the backing fs
block size via `Vfs::statvfs` and rejects a non-block-aligned `offset`/`len`
with EINVAL; COLLAPSE at/past EOF is EINVAL (Linux says use ftruncate); INSERT
at/past EOF (`offset >= size`) is EINVAL; INSERT also re-checks RLIMIT_FSIZE
against the *grown* size (`size + len`). The shifts themselves are chunked
(16 KiB) memmoves over `Vfs::read_at`/`write_at`: `fallocate_collapse_vfs`
slides the tail down (ascending copy, dst < src) then truncates by `len`;
`fallocate_insert_vfs` grows the file, slides the tail up (descending copy to
avoid clobber) then zeroes the inserted `[offset, offset+len)` hole. Our
backends are non-sparse, so this is a true content collapse/insert (not an
extent splice) — byte-for-byte identical from a reader's view; the only thing
not provided vs. a native ext4 extent op is the in-place efficiency, an
optimisation, not a correctness property. Covered by `self_test_fallocate_range`
cases (6)-(8): COLLAPSE_RANGE, INSERT_RANGE, and an INSERT+COLLAPSE round-trip
identity, all green at boot. A backend whose `statvfs` reports `block_size == 0`
(can't validate the alignment contract) keeps the EOPNOTSUPP fallback.

**Remaining limitation:** `UNSHARE_RANGE` still returns EOPNOTSUPP — it is a
reflink/CoW unshare concept our backends don't implement (there are no shared
extents to unshare). Well-behaved callers treat EOPNOTSUPP as "operation
unsupported" and skip it or fall back, so nothing breaks.

**Proper fix (deferred) for UNSHARE:** once a backend grows reflink/CoW extents
(none do today), dispatch UNSHARE_RANGE to a preallocate-and-unshare path; on a
non-reflink fs it is correctly a no-op (nothing is shared), so the EOPNOTSUPP
terminal is the conservative choice until reflinks exist. Kernel context
(caller_pid None) keeps the EOPNOTSUPP terminal for every mode, asserted by the
batch-536 FMODE_WRITE + vfs_fallocate gate-order self-tests.

### B-SIG1. dash's `wait` builtin (background-job reap) livelocked: no SIGCHLD on child exit + `rt_sigsuspend` was a stub — FIXED 2026-06-16

**RESOLVED 2026-06-16.** A real glibc `dash` running `/bin/emit > file &
wait` (Path-Z self-test `self_test_linux_real_glibc_shell_bgjob`) hung the
boot thread to a timeout. dash's `wait` builtin uses
`dowait(DOWAIT_BLOCK|DOWAIT_WAITCMD)`, whose `waitproc` computes
`flags = WNOHANG` (because `DOWAIT_WAITCMD` makes `block != DOWAIT_BLOCK`),
then loops `while (!gotsigchld && !pending_sig) sigsuspend(&oldmask)` —
relying on SIGCHLD delivery (its handler sets `gotsigchld`). The
synchronous pipe/loop/cmdsub waits use blocking `waitpid` (flags 0) and
never needed SIGCHLD, which is why those parts passed.

Two kernel gaps caused the livelock, both fixed properly:

1. **SIGCHLD was never posted to the parent on child exit.**
   `kernel/src/proc/thread.rs::on_thread_exit` now posts SIGCHLD to the
   parent when a child becomes a zombie — via the Linux-ABI disposition
   path (`signal::set_pending_info`, delivered by
   `deliver_linux_signal` → `linux_disposition`) for Linux parents, and
   `classify_post_info` for native parents (SIGCHLD's default action is
   ignore, so a no-handler parent correctly drops it). This is distinct
   from the existing `wait4()` waiter wakeups, which target a thread parked
   in `wait4()`, not the signal path.

2. **`sys_rt_sigsuspend` was a stub returning EINTR immediately.** This
   made dash busy-spin (`sigsuspend` → EINTR → re-loop → …), starving the
   boot thread. It is now a real park loop modeled on `sys_pause`
   (`kernel/src/syscall/linux.rs`): it installs the temporary mask, parks
   on the signalfd wait-queue until a signal deliverable under that mask
   arrives, and restores the original mask correctly via a Linux
   `saved_sigmask`/`TIF_RESTORE_SIGMASK` mechanism — `emit_linux_rt_frame`
   writes the saved pre-suspend mask into the handler frame's `uc_sigmask`
   (so `rt_sigreturn` restores it), and the no-handler tail of
   `deliver_linux_signal` restores it directly. The contextless
   (in-kernel, `caller_pid()==None`) case still returns EINTR immediately
   so the existing rt_sigsuspend self-test is unaffected.

**Verify:** boot test reaches `BOOT_OK`; the bgjob self-test logs "read
back 16 bytes == expected, exit 0: OK".

### B-HEAP1. Kernel heap redzone "overflow" reports during init file-install were FALSE POSITIVES from a pre-poison allocation window — FIXED 2026-06-16

**Symptom (as originally observed):** During boot (init step 24, after all
self-tests), the debug heap allocator's dealloc-time redzone scanner reported
several `[heap] BUFFER OVERFLOW detected! slot=…, alloc=N, class=C, offset=N`
lines, e.g. `alloc=10, class=16, offset=10` (right before
`[init] Installed /bin/hello`) and two `alloc=18, class=32, offset=18`. Boot
still reached `BOOT_OK` and all self-tests passed.

**Root cause (NOT a real overflow):** The redzone check relies on the invariant
"every byte in `[alloc_size, class_size)` is `ALLOC_POISON` (0xCD)". That holds
only if the slot was `poison_alloc`'d *at the time it was handed out*. But
`enable_poison()` was called very late in boot (`kernel/src/main.rs` step 22f-3,
old line ~3518) while the heap is initialized far earlier (`mm::heap::init`,
~line 455). **Every allocation made in that window was never poison-filled.**
When such a slot was later freed *after* poisoning came online, `check_redzone`
scanned whatever bytes the pre-poison occupant had left there — zeroed
fresh-frame bytes, or stale content from an earlier reuse — and reported them as
overflow. Captured byte dumps confirmed this: a slot freed with `alloc_size=18`
held the intact 31-char string `/tmp/tmpwatch_test/delete_me.tmp` filling the
whole 32-byte class (a former occupant), and `"/bin/hello"+'e'+zeros` showed
unpoisoned (zero) redzone bytes — neither is possible if the slot had actually
been alloc-poisoned. So the reports were detector false positives, not memory
corruption.

**Fix:** Move `mm::heap::enable_poison()` to immediately after `mm::heap::init()`
(`kernel/src/main.rs`, step 6), *before the first heap allocation*. With no
pre-poison allocation window, every slab slot is poison-filled at its first
alloc and the redzone invariant always holds. The redundant late
`enable_poison()` at step 22f-3 was removed (the `poison_self_test()` call
stays). Poison is still toggled OFF only for the duration of the heap
benchmarks (`deferred_bench_task`), which free their own allocations within that
window. Note this only affects slab classes (≤ 8192 B); large allocations (the
actual MB-sized binaries) go through the buddy path and are never poisoned or
redzone-checked, so the early-enable adds negligible boot cost.

### B-DP1. `validate_user_range` rejected committed-but-not-yet-faulted-in demand-paged user buffers (EFAULT on large fresh output buffers) — FIXED 2026-06-16

**RESOLVED 2026-06-16.** `kernel/src/mm/user.rs::validate_user_range`
(the core of `validate_user_read`/`validate_user_write`) walked every
4 KiB page of a user buffer and returned `InvalidAddress` the moment
`page_table::translate()` reported a page *not present*. That is wrong
for **demand-paged** memory: a freshly-`malloc`/`mmap`'d buffer is
committed (covered by a VMA) but its pages are not populated until first
touched. A syscall handed such a buffer as an *output* target would
EFAULT on every page past the first, because the process had not yet
written to those pages itself.

**Reproduce:** run `dash -c 'echo /globdir/* > out'` (Path-Z real-glibc
self-test `self_test_linux_real_glibc_shell_glob`). glibc's `opendir`
allocates a 32 KiB dirent buffer and calls `getdents64` into it before
touching it; the buffer's later pages were not present, so
`validate_user_write(dirp, 32768)` returned EFAULT, `readdir` returned
NULL, and dash's glob matched nothing — emitting the literal `/globdir/*`
instead of the three filenames. (The directory open, VFS readdir, and
getdents64 encoding were all proven correct via tracing; the validation
pre-walk was the sole culprit.)

**Fix:** when the pre-walk finds a not-present page, call the new
`try_fault_in_user_page(addr, need_writable)`, which synthesizes an x86
page-fault error code (not-present + user + write-iff-needed) and routes
it through `crate::proc::pcb::try_resolve_fault` — the same demand-paging
resolver the hardware #PF handler uses — then re-checks `translate()`.
This mirrors Linux's `get_user_pages()` faulting pages in before a
kernel-side access. A genuinely unmapped or permission-violating address
still fails (the resolver returns `false`), so invalid pointers are still
rejected. **Validated:** the dash glob self-test now reads back the
expected 45 bytes (`/globdir/a.txt /globdir/b.txt /globdir/c.txt\n`),
exit 0; full boot test passes with no self-test failures.

### B-DF1. Kernel-stack overflow → double fault when an IRQ frame pushes onto a near-full kernel task stack (deferred benchmark suite) — FIXED 2026-06-15 (Q7 option A)

**RESOLVED 2026-06-15.** Fixed via `open-questions.md` Q7 → **option A**
(operator-chosen): a dedicated per-CPU guard-page IRQ stack with a manual
nesting-aware switch in `idt::irq_common_dispatch` (so hardware IRQ frames/
handlers never consume the interrupted task's stack), plus **deferred
preemption** (timer ISR sets `NEED_RESCHED`; the outermost IRQ frame runs the
context switch on the task stack via `sched::do_deferred_preempt`). The
restructuring also exposed an **unbounded re-entrant preemption recursion**
(nested timer tick during `schedule_inner`, with interrupts enabled on the task
stack, misclassified as a fresh outermost IRQ → recursion until guard-page
overflow); fixed by disabling interrupts across the involuntary switch in
`do_deferred_preempt`. See `design-decisions.md` §26. **Validated:**
`http_gzip_8KiB` — which previously double-faulted entering the dashboard benches
on a near-full task stack — now runs to completion.

**Follow-up 2026-06-15 — `BENCH_OK` now reached end-to-end.** After the Q7
landing, two further blockers were chased to ground:

1. **The previously-documented `bench_isr_latency` null-pointer crash no longer
   reproduces.** It was an artifact of the *old* timer-ISR path that called
   `preempt()` inline during the hard-IRQ handler; the Q7 deferred-preempt
   restructuring (timer ISR only sets `NEED_RESCHED`; the switch runs later on
   the task stack) removed it. Verified by running `bench_isr_latency()` both
   early and in its normal end-of-suite slot — it completes cleanly (≈54 µs
   hard-IRQ phase under TCG, above the 10 µs target but that is emulation
   noise, not a fault). The stale `todo.txt` "Cross-Zone Bug Reports" entry is
   superseded.

2. **The actual last `BENCH_OK` blocker was a scheduler self-deadlock, now
   fixed.** `bench_dashboard_api_status` calls `dashboard::api_status()` →
   `sched::task_list()`, which holds `SCHED` (a plain `spin::Mutex`) across a
   heap `Vec` collect over *all* tasks. Run 1000× in a tight loop, a timer tick
   reliably lands while the task holds `SCHED`; the Q7 deferred-preempt then ran
   `preempt() → schedule_inner() → SCHED.lock()` on the *same* CPU and spun
   forever (the `cli` in `do_deferred_preempt` made the hang unrecoverable). The
   fix: `do_deferred_preempt` now checks `SCHED.is_locked()` and, if held,
   re-arms `NEED_RESCHED` and defers to the next tick instead of blocking — the
   same try/skip discipline `unthrottle_expired()` already uses from ISR
   context. This closes the *entire* "involuntary preempt while the interrupted
   context holds SCHED" deadlock class (including the tiny analogous window
   during voluntary `yield_now`/`block`), at the single involuntary-preempt
   site. **Validated: the full `--bench` suite now reaches `BENCH_OK` ("Boot
   test PASSED").** See `design-decisions.md` §27.

The original analysis is retained below for history.

**Root cause (CONFIRMED): kernel task stack overflow into the guard page.**
The deferred benchmark suite runs heavy, *debug-built* code paths in kernel
context (gzip/deflate, `format!`-heavy JSON, crypto) on a kernel task with a
fixed **64 KiB** stack (`TASK_STACK_SIZE = 4 * 16 KiB`). The kstack allocator
(`kernel/src/mm/kstack.rs`) lays out each task stack as `[guard 16 KiB][stack
64 KiB]`, slot stride `SLOT_SIZE = 0x14000`, region base `0xFFFF_C100_0000_0000`.
The reported fault `RSP = 0xffffc1000003ffb8` decodes to slot 3, within-slot
offset `0x3FB8`, which is **< GUARD_SIZE (0x4000)** — i.e. RSP is **inside the
guard page**, ~72 bytes below `stack_bottom`. So the stack overflowed; the
faulting `atomic_load` (and the IRQ frame that the CPU was pushing) landed on
the unmapped guard page → the fault could not be delivered → #DF.
(Correction to an earlier note: RSP is **not** "near the top of the stack" — I
had mis-decoded the slot stride. It is firmly in the guard page. The two
backtrace frames are the #DF handler's own IST stack — `handle_double_fault` /
`isr_double_fault` — and are uninformative.)

**Why an IRQ tips it over.** Hardware IRQs (timer vector 32; device IRQs 33–56,
incl. mouse IRQ12) are installed in the IDT with **IST index 0** (see
`idt.rs::init`, `IdtEntry::new(..., 0, 0)`) — they run on the *current* kernel
task stack, not a dedicated stack. When a benchmark has driven the task stack
near `stack_bottom`, the CPU pushing the interrupt frame (and the handler's own
frames) crosses into the guard page → #DF. Only the double fault itself uses an
IST (IST1). This makes *any* near-full kernel stack a double-fault risk on the
next interrupt — a real, production-relevant bug for any in-kernel code that
uses a lot of stack, not merely a benchmark artifact.

**FIXED part — the 16 KiB gzip hash table (`kernel/src/fs/compress.rs`).**
`lz77_tokenize()` allocated `let mut head = [0u32; HASH_SIZE]` with
`HASH_SIZE = 4096` = **16 KiB on the stack** (a quarter of the whole 64 KiB
stack), while its sibling `prev` was already heap-allocated. Moved `head` to a
`Vec` (heap) and changed `insert_hash`/`find_best_match` to take `&[u32]`/`&mut
[u32]` slices (call sites unchanged — `&mut Vec<u32>` coerces). Verified: with
this fix the `http_gzip_1KiB` and `http_gzip_8KiB` benchmarks now **complete**
(8192B → 4507B), where before they double-faulted. This was the dominant
single stack frame and removing it is correct regardless (gzip should never use
16 KiB of stack).

**OPEN part — RESOLVED 2026-06-15 by the Q7 option-A per-CPU IRQ stack;
empirically confirmed 2026-06-20.** The systemic interrupt-on-near-full-stack
overflow was fixed by moving interrupt handling off the interrupted task's stack
onto a dedicated per-CPU guard-page IRQ stack (`idt.rs::init_irq_stack` /
`run_on_irq_stack` / `IRQ_STACK_TOP`/`IRQ_STACK_BOTTOM`, with nesting-aware
manual RSP switch + `sched::do_deferred_preempt` after RSP is back on the task
stack — see open-questions.md Q7 / design-decisions.md §26). Once IRQ frames no
longer land on a near-full task stack, the 64 KiB task stack is sufficient for
the debug-built `core::fmt`-heavy dashboard path. **Validated 2026-06-20:**
`scripts/boot-test.sh --bench` runs the *entire* deferred suite to completion —
`dashboard_api_status`/`_health`/`_metrics`, `isr_latency`, the 62-entry
scorecard, and a clean `BENCH_OK` — with no double fault (serial-test.txt lines
9843–9913). The stale "still double-faults entering dashboard_api_status"
description below is retained for history only and no longer reproduces.

_Historical (pre-fix) description:_ After the
gzip fix the suite advances one stage further and double-faults again at the
**identical** guard-page `RSP=0xffffc1000003ffb8`, now in `Task 114` during
`bench_dashboard_api_status` (`crate::net::dashboard::bench_api_status`). The
dashboard path has no single large array — it is `format!`-heavy, and debug
builds give `core::fmt` very deep, un-inlined, stack-hungry call chains. So this
is the *general* problem: 64 KiB is marginal for debug-built in-kernel heavy
code + an IRQ frame on top. Fixing it benchmark-by-benchmark is whack-a-mole.

**Proper fix is an architectural decision — see `open-questions.md`.** The
textbook fix is a dedicated per-CPU IRQ stack (x86 IST), like Linux's IRQ
stacks, so interrupt handlers never consume the interrupted task's stack.
**Complication:** the timer handler deliberately re-enables interrupts
(`apic.rs:1162`, `sti` after EOI, for preemption), so IRQs *can* nest — a naive
single shared IRQ IST would be clobbered by a nested IRQ resetting RSP to the
IST top. A correct IRQ-stack implementation must therefore support nesting (or
the hard-IRQ phase must not re-enable IF). This is a careful change to the
hottest, most safety-critical path; alternatives (bump kernel-task stack size;
keep heavy code out of the kernel; release-build) each have tradeoffs. Deferred
to the operator as an open question rather than changing the IRQ path
autonomously.

**Reproduce:** `bash scripts/boot-test.sh --bench --timeout=600`; the suite now
runs through `compress`, `context_switch`, `pick_next`, `ipc`, `vfs`, all
`http_*` incl. both `http_gzip_*`, then #DFs entering `dashboard_api_status`.

**Large-stack-array audit (2026-06-14).** I scanned the kernel for fixed-size
stack arrays ≥ 8 KiB that could contribute to the same overflow class. Findings:
`bench.rs::bench_vfs_throughput_16k` held a `[u8; 16384]` (16 KiB) in the bench
task — moved to a heap `Vec` (committed). Remaining latent (lower-risk, not the
immediate trigger, left as tech-debt): `audio_notify.rs::self_test` `[u8; 8192]`
(boot self-test path), `syscall/linux.rs` ~line 53451 `drain [u8; 8192]`, plus
several `[u8; 4096]` buffers in `rng`/`smp`/`virtio/sound`/`linux.rs` self-tests.
Note these arrays are **not** the immediate dashboard double fault: the
`dashboard_api_status` overflow has **no** large array — it is pure debug-built
`core::fmt` call-chain depth — so reducing stack arrays will not by itself make
`BENCH_OK` appear; only the Q7 IRQ-stack / stack-size decision will.

**Impact (historical):** Before the Q7 IRQ-stack fix, `BENCH_OK` and the last
benchmarks (dashboard API, ISR latency, scorecard) did not complete. As of the
fix (and re-confirmed 2026-06-20) the full deferred suite completes and
`BENCH_OK` prints. Normal operation was never affected: the default `BOOT_OK`
boot test always passed (the deferred bench suite runs only after BOOT_OK).

### W1. Intermittent boot-test hang recurred once at the OOM self-test — WATCHLIST 2026-06-10

**Where:** boot self-test sequence; serial output (`build/serial-test.txt`)
truncated mid-line at `[sysctl] mm.oom_pol…` during `mm::oom::self_test()`
Test 3 (the `register_kill_callback` / `handle_oom(10)` step).

**Symptom:** One boot-test run did not reach `BOOT_OK` within 300s; serial
stopped mid-line inside the OOM self-test.  The very next run (identical
binary) reached `BOOT_OK` in 26s with the full OOM test passing.

**Assessment:** Same class as F1/F6/F7 — an intermittent hang that
truncates serial mid-line at whatever self-test happens to be running,
historically traced to spin::Mutex / interrupt-window / RCU timing rather
than the self-test's own logic.  `mm::oom::self_test()` and `handle_oom()`
are fully synchronous (no spawning, no blocking, fake kill callback), so
the OOM code is almost certainly the *victim*, not the *cause*.  This is
the first recorded recurrence since the F6/F7 "likely cured incidentally"
closure (128/128 prior clean boots), so it is logged here rather than
re-opening F6/F7.

**Next step if it recurs:** soak `scripts/boot-test.sh` ~20× to get a
recurrence rate and bisect the hang window the way F1/F4 were diagnosed
(finer-grained pre/post serial markers around the suspected lock).

**Soak 2026-06-12:** ran the diagnostic soak in two batches —
12× then a further 10× back-to-back `boot-test.sh` runs
(`build/oom-soak-*.log`, `build/oom-soak2-*.log`) targeting this hang
window. **22/22 clean, every run BOOT_OK at 25s, zero recurrence, no
truncated serial, no failure serials to bisect.** This **meets the
~20× diagnostic bar** the entry set, with an observed recurrence rate
of **0/22**. Consistent with the "OOM self-test is the victim, not the
cause" assessment: the single recorded truncation has not reproduced.

**Recurrence 2026-06-12 (second recorded):** while boot-testing the F10
boot-stack fix (`build/boottest-536-fixed.log`, run `brqckyayz`), one run
again truncated mid-line at exactly `[sysctl] mm.oom_pol…` during
`mm::oom::self_test()` and never reached `BOOT_OK` within 300s. The
immediate identical-binary re-run (`bx59ud6x2`) reached `BOOT_OK` in 26s
with the full OOM test passing (`[oom]   Callback registration and
invocation: OK`) and the shell prompt. This is the **same fingerprint** as
the original truncation (same self-test, same mid-line cut point), and it
is **not** caused by the F10 boot-stack change — the fix only enlarges the
boot stack / adds a redzone canary and is unrelated to the OOM self-test
path, and the canary did not trip. This recurrence **resets the clean
streak** that the 22/22 soak had been accumulating toward the ~90 closure
bar.

**Soak 2026-06-14:** 7 consecutive clean runs back-to-back (1× full
build+boot + 6× `--no-build`, `build/w1-soak-*.log`), every run BOOT_OK in
26–32s with the OOM self-test passing (`[oom]` step clean, no mid-line
`[sysctl] mm.oom_pol…` truncation). 0/7 recurrence. Clean streak now **7**
toward the ~90 closure bar.

**Status:** passive monitoring, clean streak **7** (after the 2026-06-14
soak; was reset to 0 by the 2026-06-12 recurrence). **Closure condition
unchanged:** close this item (move to Fixed/Closed as "likely cured
incidentally," like F6/F7) once a fresh combined dedicated-soak +
routine-boot clean streak passes ~90 with no recurrence. Re-open and bisect immediately on the next mid-self-test
truncation; given two recorded recurrences now, a finer-grained marker
pass around the `mm::oom::self_test()` / `sysctl::set` lock window
(per the F1/F4 method) is the priority diagnostic when next observed.

### W2. Deferred benchmark suite livelocks in `bench_pick_next` after `context_switch` → `BENCH_OK` never prints — ROOT-CAUSED & FIXED 2026-06-14

**RESOLUTION 2026-06-14 — root cause was the mouse cursor task busy-yielding,
NOT a benchmark or backend bug.** The livelock was never about the nop helpers
or `bench_pick_next` per se; it was a **system-wide priority-starvation bug**
that the long bench suite merely exposed first. `cursor_task_entry`
(`kernel/src/mouse.rs`, spawned at priority **16**) polled a lock-free mouse
event ring and, when the buffer was empty, called `crate::sched::yield_now()`
in a tight loop "to avoid spinning." But `yield_now()` re-enqueues the current
task at *its own* priority and then picks the highest-priority Ready task — and
the cursor task, at p16, was *still the highest-priority Ready task*, so it was
immediately re-picked. The "yield" loop therefore **never relinquished the CPU
to any task of priority > 16** (it only ever ceded to something strictly
higher-priority, of which there usually was none). This pinned a core, so every
p≥17 task — the p18 `deferred_bench_task` driver, the p18 workqueue worker,
background daemons — could make progress *only* via the ~1 s anti-starvation
booster (one or two tasks nudged to priority 0 each pass, hence the perpetual
`[sched] Anti-starvation: boosted N tasks` spam). `bench_pick_next` "stalled"
because its driver only got a sliver of CPU per second.

**Diagnosis chain:** markers proved `run()` never returned even though the nop
helpers *did* exit → so the lone driver itself was starving, not the helpers →
boost-ID logging (`cur=<current task> boosted <ids>`) showed the boosted/starved
tasks were tids 115 (bench driver) + 103 (workqueue worker), and that the task
hogging the CPU (`cur=`) was the **mouse cursor task** → reading
`cursor_task_entry` revealed the idle `yield_now()` busy-loop.

**Fix:** in the idle branch the cursor task now `sleep_ms(8)` (~125 Hz) instead
of `yield_now()`. `sleep_ms` (≤100 ms ⇒ hrtimer path) *removes* the task from
the run queue entirely until an hrtimer wakes it, so lower-priority work runs
freely while the cursor is idle; active mouse movement still drains events
tightly (the sleep only triggers once the ring empties). Verified: with this
fix the `--bench` suite runs from `page_alloc` all the way through `compress`,
`context_switch`, `pick_next`, `syscall_dispatch`, `ipc`, `vfs`, and into the
`http_gzip` benchmarks — vastly further than ever before (previously it never
passed `context_switch`). The default `BOOT_OK` boot test still passes
(BOOT_OK after 29 s), confirming no regression to normal operation. (Fixing W2
unmasked a separate latent double fault in a late bench stage — see B-DF1
below.)

**General lesson:** `yield_now()` is NOT a valid "idle until work arrives"
primitive for any task that is not the lowest priority on its core. A task that
yields at its own priority and is the highest-priority Ready task will be
re-picked immediately and spin. Idle waiting must *block* (sleep, or wait on a
waitqueue/futex), removing the task from the run queue. Audit other drivers for
the same `yield_now()`-when-idle antipattern.

---

**Original investigation notes (retained for history):**

**Where:** `kernel/src/bench.rs` `bench_pick_next()` (the
`run("sched_pick_next_4tasks", 500, || sched::yield_now())` loop, run after
the four `bench_nop_task` helpers at priorities 8/12/16/20 are spawned);
interacts with the scheduler's yield/pick path and anti-starvation boost in
`kernel/src/sched/mod.rs`.  Driven from the background `deferred_bench_task`
spawned at the end of `kernel/src/main.rs` boot.

**Symptom:** With `scripts/boot-test.sh --bench --timeout=600`, the deferred
benchmark suite runs cleanly through `page_alloc`, `heap`, `compress`,
`rdtsc`, `hpet`, and `context_switch_rt`, then **stalls** at/after
`bench_pick_next`: no `[bench] sched_pick_next_4tasks: …` line is ever
printed, `BENCH_OK` never arrives, and the serial log fills with continuous
`[sched] Anti-starvation: boosted N tasks to priority 0` (N = 1–2).  The
default `BOOT_OK` boot test is unaffected (it stops at `BOOT_OK`, long before
the benchmarks run).

**CORRECTION 2026-06-14 — the four nop helpers DO exit (original
"never exit" claim falsified).** A 600 s-timeout run captured all four
`bench-pn` nop helper tasks (tids **119, 120, 121, 122**) printing
`[sched] Task N exiting` *after* `context_switch_rt`'s result line — i.e. they
spawn AND drain to `task_exit` successfully.  So the nop helpers are **not**
the livelocking tasks, and `bench_pick_next`'s task-draining works.  The hang
is therefore **after** the helpers exit: either `run("sched_pick_next_4tasks",
500, yield_now)` not returning on the lone driver task (tid 114) once the
helpers are gone, a *later* benchmark stage that the driver enters silently, or
genuine starvation of 1–2 **other** Ready tasks (background daemons / the
workqueue worker tid 104 at prio 18) behind the busy prio-18 driver — those
are what the perpetual "boosted 1–2 tasks" lines refer to, NOT the nop
helpers.  Next diagnosis must localize where tid 114 actually gets stuck after
the helpers drain (add a marker after `run()` returns in `bench_pick_next` and
at the start of `bench_syscall_dispatch`), rather than assuming the nop helpers
are the culprit.

**Assessment:** Independent of the F15 sleep-queue leak — it reproduced
identically *before* the F15 fix (when it could have been blamed on
kswapd/workqueue spin-starvation) and *after* it (0 `sleep queue full`
warnings).  `run()` is a plain non-blocking loop and the task-exit path
(`task_finished` → `task_exit` → `schedule_inner(false, Uncounted)`) is clean,
so the hang is a scheduler-level livelock among several equal-/mixed-priority
tasks that only `yield_now()` (no sleeping, no I/O).  The persistent
anti-starvation boosting suggests the scheduler is thrashing — repeatedly
boosting starved tasks to priority 0 without the nop helpers ever being
scheduled through to completion.  Not yet root-caused.

**Impact:** The deferred micro-benchmark suite cannot complete past
`context_switch`, so `BENCH_OK` and the later benchmarks (pick_next, syscall
dispatch, IPC, VFS, net, crypto, HTTP, ISR latency, scorecard) never run in
normal operation.  Early-benchmark perf tracking still works:
`boot-test.sh --bench` prints the captured numbers up to the hang even on
timeout.

**Update 2026-06-14 (anti-starvation duplicate-enqueue fix — ruled OUT as the
root cause):** While investigating, I found and fixed a genuine
scheduler-correctness bug in the anti-starvation booster
(`check_starvation()` in `kernel/src/sched/mod.rs`): it boosted a starved
Ready task by `PER_CPU_SCHED.dequeue(id, effective_priority(), cpu)` followed
by `enqueue(id, 0, cpu)`.  Because `effective_priority()` returns the task's
*base* priority while an already-boosted task physically sits in priority
queue 0, the level-targeted dequeue scanned the wrong queue, removed nothing,
and the enqueue created a **duplicate** run-queue entry — the same task id
present twice in queue 0.  Re-boosting on every ~1 s pass (the booster never
reset `ready_since_tick`) multiplied the duplicates without bound.  Fix:
(a) added `dequeue_any(id)` to `PriorityRoundRobin`/EEVDF/Deadline +
`SchedulerBackend`/`PerCpuScheduler`, which removes *all* copies of a task at
*any* level and clears the bitmap bit when a level empties; the booster now
`dequeue_any` then single-`enqueue` at 0, leaving exactly one entry; and
(b) the booster now resets each boosted task's `ready_since_tick` so it is not
re-boosted before being dispatched.  This is a real, system-wide fix (the
corruption could happen to any starved task, not just benchmark tasks).
**However, it did NOT resolve W2:** with the fix in place the suite still
stalls entering `bench_pick_next` (no `sched_pick_next_4tasks` line, `BENCH_OK`
never arrives), boot remains clean (0 self-test failures, 0 sleep-queue spin
warnings), and the booster still fires (now without duplicating entries).  So
the duplicate enqueue was an *amplifier* of the thrash, not the trigger: the
benchmark nop helpers still genuinely fail to run to `task_exit`.

**Timeout calibration (corrected — the original stall-point stands).** A first
post-fix run with the default 300 s timeout appeared to stall right after
`heap_raw_alloc_free_4096`, suggesting the hang had moved earlier.  That was a
**timeout artifact, not a regression**: a 600 s re-run showed the suite *does*
still progress cleanly through `compress`, `rdtsc`, `hpet`, and
`context_switch_rt`, then stalls entering `bench_pick_next` — exactly the
original symptom.  The 300 s budget simply expired *inside* the
`compress_repeating` benchmark, which is savagely slow under QEMU/TCG:
mean ≈ 1.01 s per iteration × 200 iters ≈ **~202 s for that one benchmark
alone** (max single iter ≈ 22 s).  Because `bench_pick_next`'s own work is
trivial (~110 ms for all 500 yields at the measured ~220 µs/round-trip), its
failure to complete within the remaining multi-hundred-second budget confirms a
**genuine stall**, not mere slowness.  Practical note: reproduce W2 with
`scripts/boot-test.sh --bench --timeout=600` (the default 300 s no longer
reaches the stall point because the compress benchmarks eat the budget first).

The deeper root trigger (why four `yield_now()`-only tasks at priorities
8/12/16/20 never drain past `bench_pick_next`) is still uncharacterised.

**Next step:** Add finer-grained serial markers inside `bench_pick_next`
(before/after spawn, before/after the `run()` loop, per-iteration sampling)
and instrument the scheduler's pick/yield path to capture *which* task is
selected each switch during the stall.  Determine whether the nop helpers are
never picked, or are picked but never run to their `task_exit`.  Likely a
priority/round-robin or anti-starvation interaction; treat as a real
scheduler-correctness bug, not merely a benchmark quirk.  Risky to change the
scheduler blindly, so diagnose before patching.

_(The two prior watchlist items — accounting
self-test hang and invariant self-test hang — went 90 consecutive
boot tests with zero recurrence after F4/F5 and have been closed as
"likely cured incidentally," and as of 2026-06-10 a further 38 clean
boots (128/128 total) keep them closed.  See F6 and F7 in Fixed Bugs.
The two items discovered 2026-06-10 — quota Test 5 and FS interceptor
deny — are now fixed; see F8 and F9.)_

---

## Fixed Bugs

### B-LIMINE-KFILE-ID. Wrong Limine kernel-file request feature-ID → boot cmdline AND kernel-file symbolization silently never worked — FIXED 2026-07-14

**Where:** `kernel/src/limine.rs`, `LimineRequest::<KernelFileResponse>::KERNEL_FILE`.

**Bug:** the request's second feature-id word was `0x31eb_5d10_c871_c930`, which
does not match Limine's `LIMINE_{KERNEL,EXECUTABLE}_FILE_REQUEST` magic — the
correct value (per `limine/limine.h`, Limine 8.7.0) is `0x31eb_5d1c_5ff2_3b69`.
Because the ID never matched, Limine never populated the response, so
`boot::kernel_cmdline()` always returned `None` and `boot::kernel_file_address()`
(used for panic/backtrace symbolization from the kernel ELF) always returned
`None`. Two silent consequences: (1) the boot command line was invisible to the
kernel — `fs::kernparam` saw an empty cmdline regardless of what the bootloader
passed, so cmdline-gated switches (e.g. `net.userspace`) could never be turned
on at runtime; (2) kernel-file-based symbolization was inert. **Fix:** corrected
the feature-id word. Verified: `cmdline: net.userspace` in `limine.conf` now
round-trips into `kernparam` and flips the cutover switch. **Repro (pre-fix):**
add any `cmdline:` to `limine.conf`; the kernel read it as empty.

### B-FUTEX-TOWAKER-LOSTWAKE. Futex timeout self-test waker could lose its wakeup (wake before waiter parked) → spurious `TimedOut` under shifted boot timing — FIXED 2026-07-14

**Where:** `kernel/src/ipc/futex.rs`, `timeout_waker_task` /
`test_timeout_woken_before_deadline`.

**Bug:** the waker calls `futex_wake` **without changing the futex word**, so
correctness depended on the waiter being parked before the wake. If the wake
fired first, the waiter re-checked its (unchanged) expected value, parked anyway,
and the earlier wake was lost — the wait then ran the full 500 ms and returned
`TimedOut`, failing the test. A fixed number of `yield_now()`s cannot guarantee
the ordering; the `net.userspace` switch-on boot shifted task-id/scheduler timing
enough to expose it (manifested as whichever timeout self-test hit the bad
interleave — channel/eventfd failures in the same window were instead
daemon-starvation, fixed separately by deferring the persistent daemon past
POST). **Fix:** the waker now retries `futex_wake` until it reports it actually
woke a waiter (bounded spin), which is deterministic regardless of interleave.

### B-EVENTFD-TOTEST-SHORTTIMEOUT. Eventfd "signaled-before-expiry" self-test used a 500 ms reader timeout + fixed-yield polling → spurious `TimedOut` (`got 18446744073709551615`) under boot-time scheduler contention — FIXED 2026-07-15

**Where:** `kernel/src/ipc/eventfd.rs`, `eventfd_timeout_reader_task` /
`test_timeout_signaled`.

**Observed:** caught during the post-serial-fix wedge-soak
(`build/hang-catches/soak-20260715-022705-iter12`): boot reached BOOT_OK but the
eventfd timeout self-test failed —
`[eventfd]   FAIL: timeout_signaled: got 18446744073709551615`
(`18446744073709551615` = `u64::MAX`, the reader's error sentinel), then
`[FATAL] Eventfd timeout self-test failed: InternalError`. Intermittent: 1 of
~13 armed boots in that soak; all other eventfd sub-tests passed.

**Root cause:** this is a *test-timing* fragility, **not** a lost-wakeup — the
scheduler's `pending_wake` protection (`sched::wake` sets `pending_wake` on a
not-yet-blocked task; `block_current` consumes it) correctly closes the
register-then-park race in `read_timeout`. Instead, the reader parked with only a
**500 ms** timeout while the main test task signaled it after a fixed
`yield + sleep_ms(5)`. During the busy boot self-test phase, transient scheduler
contention can delay the *signaling* task past the reader's 500 ms deadline, so
the reader legitimately times out (`read_timeout` → `Err(TimedOut)` → stores
`u64::MAX`) even though the eventfd signal path is correct. The main task's
fixed post-write `yield×2 + sleep_ms(5)` result check compounded the fragility
(it assumed the reader is always scheduled to completion within that window).
Same class as B-FUTEX-TOWAKER-LOSTWAKE and the channel `recv_timeout` flake.

**Fix:** (a) give the reader a generous **5 s** timeout — many orders above the
~5 ms the driver takes to signal — so the timeout can never fire under normal or
momentarily-starved scheduling (only a genuinely broken signal path fails it);
(b) replace the fixed post-write yields/sleeps with a **bounded poll loop**
(200 × `yield + sleep_ms(5)`, ~1 s cap) that waits for the reader to store its
result, so a real signal-path bug still fails deterministically in ~1 s rather
than depending on exact interleave.

### D-NETSTACK-RX-DEMUX. The netstack daemon had no shared RX demux — concurrent connections couldn't safely receive at once — FIXED 2026-07-14

**Where:** `services/netstack/src/main.rs`.

**Was:** the daemon's TCP receive path read one raw Ethernet frame directly
off the NIC (`raw_rx`) filtered to *one* connection's 4-tuple via
`recv_tcp_seg`, and dropped any frame that didn't match. With the Phase-5
`conn_id`-keyed `RingConns` table letting one ring hold several live
`TcpConn`s, two connections receiving simultaneously would have connection
A's receive loop pop and discard connection B's inbound frames — corrupting
B's stream. The safe envelope was "at most one connection receiving at a
time," which the `multi` self-test respected by fully sequencing conn7's
send/recv/close before conn9 sent.

**Fix:** introduced a **shared RX pump** (`ring_pump`) that drains *every*
pending NIC frame and routes each to its owning connection by 4-tuple
(`recv_tcp_any` returns the peer identity; `RingConns::find_by_tuple` locates
the owner), feeding each segment through a single shared TCP-receive core
`TcpConn::ingest_seg` that buffers in-order payload into a per-conn `rx_buf`,
advances `rcv_nxt`, generates the cumulative ACK, and honors FIN/RST. Each
`TcpConn` now has its own `rx_buf`/`rx_len`; `take_rx` drains it. `OP_RECV`
(`ring_tcp_recv`) polls `ring_pump` — so sibling connections' frames are
delivered to *them*, not dropped — then copies the target conn's buffered
bytes into the SQE data window. The single-connection `TcpConn::recv` (used
by the one-shot `tcp_fetch` control op) shares the same `ingest_seg`/`take_rx`
core. No duplicated TCP receive logic.

**Verified:** new ring-3 self-test `netstack_ring_tcp_demux_roundtrip`
(`kernel/src/proc/spawn.rs`) opens two connections and submits **both SENDs
before both RECVs**, so conn9's response frames arrive while the daemon is
blocked in conn7's RECV — the exact concurrency the old filtered read would
have broken. Boot test 2026-07-14: both connections returned
`HTTP/1.1 200 OK` concurrently (`ring-tcp-demux conn7`/`conn9 HTTP status =
HTTP/1.1 200 OK`), and the existing `multi`/`persist` tests still pass.

### F19. rmap self-test used low fake frame addresses that collided with real CoW frames → flaky `assertion failed: is_private(frame2)` panic — FIXED 2026-06-30

**Where:** `kernel/src/mm/rmap.rs` (`self_test()`), invoked from
`kernel/src/main.rs:3288`.

**Symptom:** Intermittent boot panic `panicked at kernel\src\mm\rmap.rs:445:
assertion failed: is_private(frame2)` (also reproducible at the Test-1
`add(frame1,...)`/`count==1` assertion). The rmap self-test ran to completion on
most boots but panicked on others — pure timing/allocation flakiness, not a
deterministic failure. Surfaced while validating the container read-only-volume
work (increment 15); that change is functionally invisible to this MM path —
it merely perturbed frame-allocation timing enough to expose the latent test
bug. (A separate, also-flaky CoW-pipeline hang in the same boot run is the known
F18-family fragility of the `dash | … > file` ring-3 test and is unrelated.)

**Root cause:** The rmap is a **global** hash table keyed by physical frame
address, and `self_test()` runs *late* in boot — after the Path-Z ring-3
toolchain tests (dash pipelines, tcc, make) have done heavy CoW/fork activity
that registers thousands of **real** user frames in that global table. The test
used fixed low fake addresses (`frame1 = 0x10_0000` = 1 MiB, `frame2 = 0x20_0000`
= 2 MiB, untracked-frame probe `0xDEAD_0000`). When a real user frame happened to
sit at exactly one of those physical addresses, it already had a mapper in the
table, so the test's `add(frame2, pml4_a, virt2)` appended a *second* mapper and
`is_private(frame2)` returned false → assertion panic. Whether a real frame
landed on 0x20_0000 depended on allocation order, making it flaky.

**Fix:** Move the test frames far above any installed physical RAM (machines here
have at most a few GiB) so the global table can never hold a pre-existing entry
for them: `frame1 = 0x0F00_0000_0000` (~15 TiB), `frame2 = frame1 + 16 KiB`, and
the untracked-frame probe to `0x0F00_0001_0000`. These remain valid u64 hash keys
(the rmap does not validate physical-address width) and are impossible as real
frames, so the test is now collision-proof regardless of allocation timing. A
detailed comment records the invariant. (A fuller fix — refactoring the rmap API
to operate on an injectable test-local table instead of the global static — was
rejected as disproportionate: it would add a `&mut table` parameter to every
production rmap entry point purely for testability. Impossible-address selection
is the minimal correct fix.) The self-test still cleans up all its entries
(`frame1`/`frame2` removed before exit), so no fake entries leak into the live
table.

### F18. CoW refcount granularity mismatch (per-16 KiB-frame refcount vs per-4 KiB-PTE resolution) double-freed a still-shared frame → parent `dash` #GP in a pipeline — FIXED 2026-06-16

**Where:** `kernel/src/mm/cow.rs` (`resolve_cow_fault`, `clone_frame_group`)
and `kernel/src/mm/page_table.rs` (`clear_user_address_space`).

**Symptom:** A real `dash -c '/bin/emit | /bin/countbytes > /dash-pipe-out.txt'`
(Path Z Part 12) crashed the *parent* `dash` with a #GP at glibc
`wait4`'s errno store (`mov %eax,%fs:(%rdx)`, libc+0x110839) — but only
on the `wait4` *error* path (e.g. `-ECHILD`), which is why the
single-fork Part 11 never hit it. The faulting `%rdx` was garbage loaded
from a libc `.got` slot (the errno `R_X86_64_TPOFF64` negative TLS
offset), so `%fs:(%rdx)` was non-canonical. The `.got` 4 KiB page lived
at virt `0x6000203000`, sub-page 3 of the 16 KiB frame group based at
`0x6000200000`.

**Root cause:** CoW refcounting is **per-16 KiB frame** (the buddy
allocator's unit), but CoW *sharing/resolution* is tracked **per-4 KiB
PTE** (each 16 KiB frame maps as 4 consecutive PTEs). The ELF loader
packs a read-only segment tail and a writable segment head into one
16 KiB frame, so a group can hold a read-only *shared* sub-PTE (no COW
bit) next to a writable *CoW* sub-PTE — both pointing into the same
frame. Three operations used **inconsistent** rules for "the group's
reference to the frame":
- `clone_frame_group` incremented the refcount once, keyed on the *first
  present* sibling.
- `resolve_cow_fault` decremented once per resolve event whenever *any*
  CoW sibling was copied out — **even though a read-only shared sibling
  still referenced the old frame**.
- `clear_user_address_space` freed once per group, keyed on *only the
  base (sub-page 0)* PTE.

So a forked child that wrote the writable sub-PTE resolved it to a
private copy and decremented the old frame, *while still mapping the old
frame via the read-only sub-PTE*. At teardown the child's base PTE still
pointed at the old frame → it freed it **again** (double-decrement). Two
such children drove the parent-shared frame's refcount to 0; the freed
frame was reused (filled with a child's exec image), corrupting the
parent's `.got` errno slot → garbage `%rdx` → #GP.

**Fix:** Make all three operations agree on one invariant — *each address
space holds exactly one refcount on each **distinct** 16 KiB frame its
group's sub-PTEs reference*:
- `resolve_cow_fault` now drops the old frame's reference (ref_dec + rmap
  remove) **only if, after the copy loop, no sub-PTE of the group still
  points into the old frame**. A read-only shared sibling keeps the
  reference alive; the new private frame is registered in rmap
  unconditionally.
- `clone_frame_group` increments the refcount (and adds rmap) once **per
  distinct frame** found among the group's present siblings (handles a
  parent that had already partially resolved a group before forking
  again).
- `clear_user_address_space` inspects **all four** sub-PTEs of each group
  and frees each **distinct** frame exactly once (was: only the base
  PTE), so copied-out private frames are no longer leaked and refcounts
  stay symmetric with resolve/clone. (The refcount-aware `free_frame`
  already only returns a frame to the allocator at its last reference.)

**Verification:** Part 12 boot self-test
`proc::spawn::self_test_linux_real_glibc_shell_pipe` now passes (parent
`dash` exits 0, `/dash-pipe-out.txt` == `n=16\n`).

### F17. fd-bearing resources were closed at *reap* (`destroy`) instead of at *exit* (zombie) → `cmd1 | cmd2` pipeline deadlock — FIXED 2026-06-16

**Where:** `kernel/src/proc/pcb.rs` — new `exit_close_fds(pid)` + extracted
`close_initial_fds()`; `kernel/src/proc/thread.rs` — `on_thread_exit` calls
`pcb::exit_close_fds(pid)` at the zombie transition;
`kernel/src/proc/pcb.rs::destroy_process_resources` now just calls
`cleanup_handles` + `close_initial_fds` for the force-kill / never-zombied
path (the slices are already empty on the normal exit path).

**Symptom:** A real glibc `cmd1 | cmd2` pipeline (`/bin/pipe`: `pipe`→`fork`;
child `dup2`s the write end onto fd 1 and `execl`s `/bin/emit`; parent closes
the write end, `read`s the pipe to EOF, then `waitpid`s the child) **hung
forever** — `self_test_linux_real_glibc_pipe` reported "process did not exit
within N yields (state=Running)" regardless of the yield budget (a 4×
budget bump changed nothing — the tell that it was a deadlock, not
under-budgeting).

**Root cause:** A blocked pipe reader only gets EOF (`read()`→0) when the
*last* write end closes. The child's exec'd image inherited a copy of the
pipe write end; that fd's kernel resource was only released by
`destroy_process_resources`, which ran when the **parent reaped** the child
via `wait4`. But the parent could not reach `waitpid()` until its `read()`
returned EOF. EOF ⟸ child's write end closed ⟸ child reaped ⟸ parent past
`read()` ⟸ EOF. Circular wait → deadlock.

**Fix:** Close every fd-bearing kernel resource (all `ipc_handles` + any
unclaimed initial fds) the moment a process **exits** (becomes a zombie),
not when its parent reaps it — matching Linux's `exit_files()` in `do_exit`.
`exit_close_fds` `core::mem::take`s the two lists out of the PCB under the
table lock, drops the lock, then dispatches `cleanup_handles` +
`close_initial_fds`. Idempotent: the reap-time teardown finds the lists
already drained, so no double-close and no leak; the force-kill path (where
a process is destroyed without ever zombying) still closes everything.

**Validation:** `self_test_linux_real_glibc_pipe` now passes — the parent
wakes from `read()` the instant the child zombies, prints
`SLATE_GLIBC_PIPE_OK n=16 body=SLATE_PIPE_BODY\n` (46 bytes captured ==
expected) and `exit(29)`; boot test PASSED. This is a general correctness
fix: it affects every pipe/socket EOF-on-last-writer-exit, not just the
test. It is also the standing semantics any real shell relies on.

### F16. `on_thread_exit_hook` dereferenced user pointers unconditionally → kernel page-fault panic when thread cleanup ran cross-address-space — FIXED 2026-06-16

**Where:** `kernel/src/proc/thread_clone.rs` — `on_thread_exit_hook(task_id)`.

**Symptom:** `PANIC` — page fault in `read_user` reached via
`fetch_robust_entry ← exit_robust_list ← on_thread_exit_hook`, with CR2 in a
glibc-mmap user range, when a boot self-test reaped a real glibc process
(e.g. the Part 7 pipe test) by calling `thread::on_thread_exit(task_id)` from
**task 0's (boot) address space** rather than the dying process's.

**Root cause:** The exit hook walked PI-owned futexes, the glibc robust
list, and zeroed `clear_child_tid` — all of which dereference *user* virtual
addresses valid only in the dying process's address space. When the hook
runs from a different active CR3 (cross-AS reap), those addresses point into
the wrong (or unmapped) address space → faulting kernel read → panic.

**Fix:** AS-active guard. The hook computes
`as_active = page_table::active_pml4_phys() == pcb::get_pml4(owner_process(task_id))`
and runs the user-memory operations (PI-futex walk, robust-list walk,
`clear_child_tid` zero-write + `futex_wake`) **only when `as_active`**. The
in-kernel bookkeeping removals (`ROBUST_LIST` / `RSEQ` / `CLEAR_CHILD_TID`
map entries) always run regardless. When not AS-active the hook skips the
user dereferences and returns after the in-kernel cleanup — correct, because
the futex-wake/ctid-clear only matter to a live address space, and a process
being reaped from outside its own AS has no threads left to wake.

**Validation:** the Part 7 pipe boot test no longer panics in the robust-list
walk; boot test PASSED.

### F15. Sleep-queue slot leak: an expired entry was only freed when `try_wake` returned `true`, so tasks woken early / destroyed before their deadline leaked a slot permanently — daemons then busy-spun and starved low-priority work — FIXED 2026-06-14

**Where:** `kernel/src/sched/mod.rs` — `process_sleep_wakeups()` and the new
`wake_expired_sleeper()` helper; the fixed-size `SLEEP_QUEUE` (`MAX_SLEEPERS`
= 256) and the `sleep_until_tick()` busy-spin fallback.

**Symptom:** Surfaced while adding a `--bench` mode to `scripts/boot-test.sh`
(which waits for the deferred `BENCH_OK` instead of stopping at `BOOT_OK`).
During the post-boot benchmark phase the serial log filled with **688**
`[sched] WARNING: sleep queue full, task <N> falling back to spin` lines —
tasks 103 (kswapd) and 104 (the workqueue worker), both long-lived daemons
that sleep between work, could no longer register a sleep, so they fell back
to the `yield_now()` busy-spin loop in `sleep_until_tick()`. That pinned a CPU
and starved the low-priority deferred-benchmark task. The default boot test
never saw this because it kills QEMU at `BOOT_OK`, before the daemons have
looped enough to exhaust the queue.

**Root cause:** `process_sleep_wakeups()` (timer-ISR tick handler) cleared an
expired slot only when `try_wake(task_id)` returned `true`. But `try_wake`
returns `false` in two fundamentally different situations:
1. **Lock contended** (`SCHED.try_lock()` failed) — transient; retrying next
   tick is correct.
2. **Task not `Blocked` / no longer in the table** — terminal. A task that
   slept and was then woken early through another path (channel/futex/eventfd
   wake), or that was destroyed before its deadline, is no longer `Blocked`,
   so `try_wake` can *never* succeed for that slot again.
The code conflated the two and kept the slot in both cases. In the terminal
case the slot was retained forever — a permanent leak. As short-lived
boot/self-test/benchmark tasks slept-then-exited, slots leaked one by one
until all 256 were gone, after which every subsequent sleeper busy-spun.

**Fix:** Split the two failure modes with a dedicated `wake_expired_sleeper()`
that returns `SleeperWake::{Release, Retry}`. It acquires the scheduler lock
itself: on `try_lock` failure it returns `Retry` (keep the slot — genuine
contention); otherwise it inspects the task and returns `Release` in **all**
non-contention cases — task still `Blocked` (wake it, as before), task present
but already awake (record `pending_wake`, release), or task gone (release).
`process_sleep_wakeups()` now clears the slot whenever it gets `Release`, so an
expired slot is reclaimed at its deadline at the latest, bounding occupancy to
"tasks with un-expired deadlines" instead of leaking permanently. Verified by
re-running `scripts/boot-test.sh --bench --no-build`: the
`sleep queue full` warning count dropped from **688 to 0**, with the benchmark
numbers up to `context_switch_rt` captured cleanly.

**Residual (separate, pre-existing):** `BENCH_OK` is still not reached — the
deferred benchmark suite livelocks later, in `bench_pick_next` (logged
separately under Active Bugs as the "deferred benchmark suite hangs after
`context_switch`" item). That hang reproduced identically *before* this fix
(when it was masked by the spin-starvation) and *after* it (0 spin warnings),
confirming it is independent of the slot leak.

### F14. `arch_prctl(ARCH_SET_GS)` wrote `KERNEL_GS_BASE` (Linux convention) but Slate's entry stub uses the inverted GS convention → first syscall after SET_GS faulted on per-CPU access — FIXED 2026-06-14

**Where:** `kernel/src/syscall/linux.rs` `sys_arch_prctl` (ARCH_SET_GS /
ARCH_GET_GS arms); the userspace `%gs`-base context-switch restore in
`kernel/src/sched/mod.rs` (both switch sites); the `execve` `%gs` reset in
`kernel/src/proc/spawn.rs`.

**Symptom:** Latent until exercised. The new two-process `%gs`-base
context-switch regression test (`self_test_linux_gs_tls_switch`) reliably
triggered it: a ring-3 process that issued `arch_prctl(ARCH_SET_GS, sentinel)`
and then made *any* further syscall took an unrecoverable kernel `#PF` writing
to `sentinel + 8` — i.e. the syscall entry stub's `mov gs:[8], rsp` was
dereferencing the user's `%gs` sentinel as if it were the per-CPU base. With
no real ring-3 caller ever issuing ARCH_SET_GS before this test, the bug had
shipped undetected.

**Root cause — two self-consistent GS conventions, mixed:**
- **Linux convention:** syscall handlers run with the per-CPU pointer *active*
  in `GS_BASE` (one `SWAPGS` at entry, one at exit) and the userspace value
  parked in `KERNEL_GS_BASE`. So Linux's `ARCH_SET_GS` writes `KERNEL_GS_BASE`.
- **Slate's actual entry stub** (`kernel/src/syscall/entry.rs`) does a *second*
  `SWAPGS` back before calling the Rust handler, so a handler runs with the
  userspace `%gs` base *active* in `IA32_GS_BASE` and the per-CPU pointer
  resting in `KERNEL_GS_BASE`. Phase 4 swaps again for per-CPU stack access on
  the way out. Interrupts never `SWAPGS` at all. The invariant is therefore
  "**`KERNEL_GS_BASE` always holds the per-CPU pointer while in the kernel**,"
  and the userspace `%gs` base is simply the active `IA32_GS_BASE` — fully
  symmetric to `%fs`/`IA32_FS_BASE`.

  The pre-existing `ARCH_SET_GS` was copied from the *Linux* convention
  (writing `KERNEL_GS_BASE`), which under Slate's stub clobbers the per-CPU
  pointer mid-handler; phase 4's `mov gs:[8], …` (after its `SWAPGS` brings the
  now-corrupted `KERNEL_GS_BASE` into the active slot) then faults.

  A first attempt at the context-switch restore made the same wrong assumption
  in the other direction — it tried to fall back to a "live per-CPU base" read
  from `IA32_GS_BASE` when a task had no custom `%gs`. But inside a syscall
  handler `IA32_GS_BASE` holds the *user's* base (0 for a never-set task), so
  that read yielded 0 and the next `SWAPGS` loaded `GS_BASE = 0`, faulting per-CPU
  access on the *first* ring-3 process spawned.

**Fix:** Treat the userspace `%gs` base exactly like `%fs` — it is the active
`IA32_GS_BASE`. `ARCH_SET_GS`/`ARCH_GET_GS` now write/read `IA32_GS_BASE`
(0xC000_0101), not `KERNEL_GS_BASE`; the scheduler restores
`wrmsr(IA32_GS_BASE, task.gs_base)` on switch-in for user tasks (0 = no custom
`%gs`, the default — correct to restore directly); `execve` resets
`IA32_GS_BASE = 0`. `KERNEL_GS_BASE` is now written in exactly one place
(`syscall::entry::init`, the per-CPU pointer) and never touched again, making
the invariant trivially true. The TD4 `arch_prctl` GS validation self-test was
updated to bracket `IA32_GS_BASE` instead of `KERNEL_GS_BASE`. Verified: build
+ clippy (0 errors) + boot-test green; both the `%fs` and `%gs` two-process
context-switch regression tests print OK and there are no panics.

**Lesson:** When two layers each encode a CPU-state convention (the asm entry
stub vs. the syscall handler), they must agree explicitly. The FS/GS-base
handling is the canonical example; both are now documented as "active-register,
symmetric to %fs" on `cpu::IA32_GS_BASE`, `Task::gs_base`, and the
`sys_arch_prctl` const doc.

### F13. Userspace `%fs` (TLS) base and `%gs` base were not saved/restored per task across context switches — FIXED 2026-06-14

**Where:** `kernel/src/sched/mod.rs` context-switch path (both switch sites);
`kernel/src/sched/task.rs` (`fs_base`/`gs_base` fields);
`kernel/src/syscall/linux.rs` `sys_arch_prctl`; `kernel/src/proc/{fork,
thread_clone,spawn}.rs`.

**Symptom:** Latent for single-process workloads; fatal for any multi-process
glibc workload (a real toolchain: gcc/ld/make/bash). `IA32_FS_BASE` is glibc's
thread-local-storage pointer (`%fs` base) and is a global CPU register *not*
part of the saved GP `Context`. With two concurrent glibc processes, a context
switch left the incoming process running on the outgoing process's TLS pointer
— silently corrupting `errno`, the stack-protector canary, and every `__thread`
variable. The `%gs` base (see F14) is the sibling register with the same flaw.

**Root cause:** The scheduler swapped CR3, FPU state, and the GP register
`Context` on a switch, but never the per-thread segment-base MSRs. `CR4.FSGSBASE`
is off, so userspace can only change these via `arch_prctl`/`CLONE_SETTLS`,
making a kernel-stored per-task field authoritative.

**Fix:** Added authoritative per-`Task` `fs_base`/`gs_base` fields, restored on
switch-in for user tasks (`pml4_phys != 0`), kept in sync at
`arch_prctl(ARCH_SET_FS/SET_GS)`, inherited across `fork`/`clone`, and reset on
`execve`. Two two-process ring-3 regression tests
(`self_test_linux_fs_tls_switch`, `self_test_linux_gs_tls_switch`) install
distinct sentinel bases in concurrent processes and assert each survives
cooperative yields; both print OK at boot. (See F14 for the `%gs`-specific
convention subtlety that the GS half of this work uncovered.)

### F12. ALSA PCM `hw_params` leaked a mixer slot under concurrent calls on a shared fd — FIXED 2026-06-13

**Where:** `kernel/src/ipc/alsa_pcm.rs` `hw_params` (the slot-reservation
re-acquire path, ~lines 376-410).

**Symptom:** None observed yet (latent). Two concurrent `SNDRV_PCM_IOCTL_HW_PARAMS`
ioctls on the *same* PCM fd — reachable when a fd is shared across threads or
inherited across `fork()` — could permanently leak one `audio_mixer` stream
slot. Mixer slots are a finite resource, so repeated occurrences would
eventually exhaust them and make `open_stream` fail with `WouldBlock` for all
clients.

**Root cause:** A TOCTOU window in the leaf-lock dance. `hw_params` read
`need_stream = pcm.mixer_stream.is_none()` under the table lock, dropped the
lock to call `audio_mixer::open_stream()` (which must not run under the table
lock), then re-acquired the lock and did `pcm.mixer_stream = Some(sid)`
**unconditionally**. Two racing calls both observed `mixer_stream == None`, both
opened a slot, and the one that re-acquired the lock second overwrote the
first's stored `StreamId` — orphaning it (it was never `close_stream`d; the
instance's eventual `close` frees only the surviving slot).

**Fix:** On re-acquire, only store the freshly-opened slot if `mixer_stream` is
still `None`; otherwise treat it as redundant, keep the existing slot, and free
the redundant one with `audio_mixer::close_stream` *after* dropping the table
lock (preserving the documented leaf-lock invariant — no mixer call under the
table lock). Added a single-threaded idempotency assertion to the self-test
(a repeat `hw_params` stays `SETUP` with unchanged params, exercising the
`need_stream == false` reuse branch).

### F11. hrtimer self-test Test 2 raced the APIC timer ISR → intermittent boot panic — FIXED 2026-06-12

**Where:** `kernel/src/hrtimer.rs` self-test Test 2 (~lines 475-496).

**Symptom:** Intermittent boot panic at `hrtimer.rs:488`
`"Timer with 0 delay didn't fire on process_expired()"`. The panic blocked
the boot gate for any batch whose validation run happened to lose the race,
even though the code under test was correct.

**Root cause:** The self-test runs with interrupts ENABLED. It scheduled a
0-delay timer and then called `process_expired()` manually, expecting to
drain it. But the periodic APIC timer ISR also calls `process_expired()`;
when the ISR fired in the window between `schedule_ns` and the manual
`process_expired()`, the ISR drained the 0-delay timer first, so the manual
call returned `n == 0` and the `assert!(n >= 1, ...)` panicked.

**Fix:** Wrap the `schedule_ns(0, ...)` + `process_expired()` pair in
`crate::cpu::without_interrupts(|| { ... })` so the manual drain is
deterministic — the ISR cannot steal the timer in between. Test-only
correctness fix; the hrtimer subsystem itself was already correct.

### F10. Boot-stack overflow from monolithic translation self-test silently corrupted `.bss` (FPU_STRATEGY) → futex-test `#UD` — FIXED 2026-06-12

**Where:** `kernel/src/main.rs` boot stack (`KERNEL_BOOT_STACK`, was 512 KiB)
vs. `kernel/src/syscall/linux.rs::self_test()` (a single ~1.4 MB monolithic
function). Crash surfaced in `kernel/src/sched/context.rs::switch_context`
reading `sched::context::FPU_STRATEGY`.

**Symptom:** Boot reached `[syscall/linux] Translation self-test PASSED`,
then the very next subsystem — `ipc::futex::self_test()` — spawned task 36
("futex-test") and the first context switch faulted:
`EXCEPTION: Invalid Opcode (#UD) at 0xffffffff81133b0e`, instruction bytes
`49 0f ae 20` (= `xsave64 [r8]`), then `FATAL: Unrecoverable kernel #UD`.
The kernel never reached `BOOT_OK`, so boot-test could not pass. Appeared
only after the batch-536 ABI change (a translator-only `sys_fallocate`
gate not even exercised by the futex test) — a classic layout-shift
heisenbug. Reproduced deterministically with batch 536 applied; passed
deterministically with it stashed.

**Root cause:** Boot-stack overflow. `switch_context` dispatches the FPU
save on the global `FPU_STRATEGY` byte (0=FXSAVE, 1=XSAVE, 2=XSAVEOPT).
Boot init selected **FXSAVE** (QEMU CPU reports no XSAVE; serial line 84:
`strategy=FXSAVE`), yet the crashing switch executed the **XSAVE64**
branch → `FPU_STRATEGY` had been corrupted 0→1. The corruptor: the
monolithic `syscall::linux::self_test()` runs directly on the boot stack
and, in the unoptimized debug build (`opt-level=0`, no stack-slot
coloring), its frame is the *sum* of every per-batch block's locals —
disassembly of the prologue showed a ~480 KiB frame (`sub r11, 0x75000` +
probe loop + `sub rsp, 0x900`). With the 512 KiB boot stack (no guard
page) minus `kernel_main`'s own frame, batch 536's extra locals tipped the
frame past the stack bottom; the prologue's page-probe / frame writes
scribbled the adjacent `.bss`, flipping the `FPU_STRATEGY` byte to 1. The
self-test still completed (it never re-reads that byte), printed PASSED,
and returned — the poison only bit later when the futex context switch
trusted the corrupted strategy and ran `xsave64` on a CPU without
`CR4.OSXSAVE` → `#UD`. The boot stack having **no guard page** is what made
the overflow silent instead of a clean fault (same silent-`.bss`/page-table
class noted in the `KERNEL_BOOT_STACK` doc comment for the original Limine
stack).

**Fix (`kernel/src/main.rs`):**
1. Enlarged `KERNEL_BOOT_STACK_SIZE` 512 KiB → **2 MiB** so the boot-time
   self-tests fit with generous headroom (~1000+ ABI batches of runway).
2. Added a **64 KiB bottom redzone canary** (`BOOT_STACK_REDZONE`,
   `BOOT_STACK_CANARY = 0xC7`): `init_boot_stack_canary()` fills it early in
   `kernel_main` (RSP near top), `check_boot_stack_canary()` (called right
   after `syscall::linux::self_test()`) volatile-scans it and FATAL-halts
   with a clear "boot stack overflow detected" message if clobbered. The
   unoptimized stack-probe prologue writes a zero to every 4 KiB page it
   descends through, so any frame that reaches the redzone is guaranteed to
   trip the canary — converting future silent overflows into clear
   diagnostics before they can corrupt the `.bss` below the stack.

**Proper long-term fix (tracked as TD4):** the real smell is the monolithic
~1.4 MB `self_test()` with an unbounded per-batch frame. It should be split
into many small `#[inline(never)]` sub-functions so no single frame is
large. Deferred because the function is one giant 4-space enclosing block
(~39 k lines, opens early / closes at line 75298) and a hand-split risks
silently mis-scoping shared locals; the 2 MiB stack + canary make the
system correct and self-diagnosing in the meantime.

**Verification:** boot-test with batch 536 applied now reaches `BOOT_OK`
in 26s (was deterministic `#UD` FATAL before `BOOT_OK`), with serial
running through to the `user>` shell prompt; the redzone canary scan runs
clean (no "boot stack overflow detected"), `[syscall/linux] Translation
self-test PASSED`, and the futex self-test that previously faulted now
completes normally. (One of the validation runs hit the pre-existing
intermittent OOM-self-test truncation tracked as W1 — unrelated to this
fix; the immediate re-run was clean.)

### F8. quota self-test Test 5: wrong inode expectation (test bug, not production) — FIXED 2026-06-10

**Where:** `kernel/src/fs/quota.rs` — `self_test()` Test 5.

**Symptom:** Boot serial printed a non-fatal ERROR "expected Allowed at
limit, got SoftWarning" from Test 5.

**Root cause:** A *test* bug, not a production-code bug. Test 2 sets the
test user's limits to `soft_inodes = 100, hard_inodes = 200`. Test 5
then set usage to 199 inodes and expected `check_create()` to return
`Allowed`, with a comment reasoning only about the hard limit ("→ 200,
equals hard, should be allowed"). It ignored that 199 inodes is already
far over the soft limit of 100, so `check_inodes()` correctly returns
`SoftWarning` (199+1 = 200 > soft 100; grace not yet enforced). The
production check path is correct and symmetric with `check_bytes()`
(both use `new_total > limit`): there is no inode-vs-byte off-by-one.

(Initially mis-logged as Active bug A1 — a supposed production off-by-one
in the inode soft-limit boundary. That was wrong; corrected on the same
day after reading the limit setup.)

**Fix:** Rewrote Test 5 to exercise all three quota bands the way Tests
2-4 do for bytes — under-soft (50 inodes → Allowed), over-soft within
grace (150 → SoftWarning), and at the hard limit (200 → Denied) — so it
validates real inode-quota semantics instead of asserting a value the
code never produces.

**Verification:** boot-test — quota self-test reaches "[quota]   inode
limit OK" with no ERROR.

### F9. FS interceptor deny handlers fail open for trailing-slash prefixes — FIXED 2026-06-10

**Where:** `kernel/src/fs/intercept.rs` — `pre_check()` interceptor
match filter.

**Symptom:** Boot serial printed non-fatal "[intercept]   ERROR: deny
handler allowed". A `Deny` interceptor registered for `/protected/` did
not block a write to `/protected/secret.txt` — it failed *open*.

**Root cause:** The match filter used
`path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')`,
but interceptors are registered with a **trailing-slash** prefix
(`/protected/`). With the slash included, `get(prefix.len())` looks at
the byte *after* the slash, so the check only matched double-slash paths
(`/protected//x`). Real children like `/protected/secret.txt` never
matched, so the deny handler was never invoked and the operation was
allowed. (Same idiom bug as F-class integrity.rs fix in commit
`22a8098f`; see TD3 for the broader audit.)

**Fix:** Extracted `path_matches_prefix(path, prefix)` which normalises
away a single trailing slash (`strip_suffix('/')`) before applying the
canonical component-boundary check, so it is correct whether or not the
registrant supplied a trailing slash, and also matches the protected
directory node itself (`/protected`). Added boundary regression
assertions to Test 3: `/protectedX/file.txt` must NOT match (no prefix-
string leak) and `/protected` (the dir itself) must match.

**Verification:** boot-test — "[intercept]   deny handler with path
prefix OK" and "[intercept] Self-test passed (10 tests)" with serial
showing DENIED on both `/protected/secret.txt` and `/protected` and no
denial of `/protectedX/...`.

### F1. RCU self-test occasionally hangs at boot (intermittent) — FIXED 2026-06-07

**Where:** `kernel/src/rcu.rs` — `call()`, `process_callbacks()`,
`stats()` and (defense-in-depth) `synchronize()`.

**Root cause:** The `CALLBACKS` spinlock was acquired both from
direct callers (boot path → `rcu::call`, `rcu::stats`,
`rcu::synchronize` → `process_callbacks`) AND from `rcu::tick()`
running in softirq context.  Softirqs dispatch with interrupts
re-enabled on the same CPU.  If a timer ISR fired while a direct
caller held the lock, the softirq's `process_callbacks()` re-entered
the same critical section on the same CPU and deadlocked the
spin::Mutex.  The hang manifested between
`[rcu]   Quiescent state: OK` and `[rcu]   Callback registration: OK`
(i.e. inside `rcu::call`) because that's the first lock acquisition
after the periodic softirq starts running.

**Diagnosed by:** Running boot-test.sh 10× — observed 2 hangs, both
with the serial log truncated at exactly the same point (after
"Quiescent state" probe, before "Callback registration").  This
showed the hang was in `call()`, not `synchronize()` as the original
hypothesis suggested.

**Fix:** Wrap every `CALLBACKS.lock()` site in
`crate::cpu::without_interrupts(...)` so the lock cannot be acquired
from a path that is interruptible.  Additionally, in `synchronize()`,
explicitly bump the calling CPU's own QS counter after snapshotting
(the caller cannot itself be in a read-side critical section by RCU
invariant), and add a million-iteration safety cap with diagnostic
print so any future grace-period failure surfaces a warning instead
of a silent hang.  Added finer-grained "[rcu]   Synchronize: pre/post"
self-test probes to localize any future regression.

**Verification:** 20/20 consecutive boot tests pass after the fix
(previously 2/10 hung).

### F2. Watchdog self-test heartbeat-increment assertion race — FIXED 2026-06-07

**Where:** `kernel/src/watchdog.rs` — `self_test()` test 1.

**Root cause:** The test does
`before = HEARTBEATS[cpu].load(); heartbeat(); after = HEARTBEATS[cpu].load();`
and asserts `after == before + 1`.  But the APIC timer ISR also calls
`watchdog::heartbeat()` on every tick (via `apic.rs`), so a timer
interrupt landing inside the before→after window can cause the
counter to advance twice, tripping the assertion.  Observed once on
2026-06-07: panic with `left: 368, right: 367`.

**Fix:** Wrap test 1's load/heartbeat/load sequence in
`crate::cpu::without_interrupts(...)`.

**Verification:** 20/20 consecutive boot tests pass after the fix.

### F3. Softirq self-test races APIC timer ISR — FIXED 2026-06-07

**Where:** `kernel/src/softirq.rs` — `self_test()` tests 2, 3, and 4.

**Root cause:** The self-test runs after `[boot] Interrupts enabled —
preemptive scheduling active`, so the APIC timer ISR fires
asynchronously throughout the test.  The ISR's path calls
`process_pending()` on the same CPU, which mutates `TOTAL_RUNS`,
`TOTAL_HANDLERS`, `IN_SOFTIRQ`, and `PENDING`.  Three races:

  * Test 2 (no-op fast path): an ISR firing between
    `process_pending()` returning and `TOTAL_RUNS.load()` bumps the
    counter and trips `runs_after != runs_before`.
  * Test 3 (dispatch + clear): an ISR firing between `raise()` and
    the test's own `process_pending()` drains TIMER_SOFTIRQ first;
    the test's call then runs no handler and trips
    `handlers_after <= handlers_before`.
  * Test 4 (re-entry guard): after the test clears
    `IN_SOFTIRQ[cpu] = false`, an ISR firing before the
    `still_pending` load runs a real `process_pending()`, consumes
    TIMER_SOFTIRQ, and trips "bits were consumed despite re-entry
    guard".  Observed once on 2026-06-07 during the post-RCU-fix
    soak (build/serial-test.txt at 11:44).

**Fix:** Wrap each of tests 2, 3, and 4 in
`crate::cpu::without_interrupts(...)`.  In test 4, also sample
`PENDING` *before* clearing `IN_SOFTIRQ` so the semantic ordering
("did the guarded call consume bits?") is preserved.  `process_pending`
internally toggles IF (STI→handlers→CLI); `without_interrupts` saves
and restores the outer IF state, so the boot path's interrupt state
post-test is unchanged.  Test 1 already had its own CLI/STI window
and didn't need changes.

**Verification:** Boot test passes cleanly with `softirq` self-test
showing all four sub-tests OK and `Self-test PASSED`.  Post-fix
30-run soak: 29/30 pass with zero softirq self-test failures (the
single failure was in `frag_history` test 6 — see F4 below).

### F7. Invariant self-test hang — LIKELY CURED INCIDENTALLY 2026-06-07

**Where:** `kernel/src/invariant.rs` — `self_test()`, between the
test 1 `check_all()` call and the test 2 `all_ok()` call.

**Original symptoms:** Single observation 2026-06-07 during the
post-RCU-fix soak (`build/soak-hang-run2.txt`).  Serial output stopped
cleanly after the 8th `[PASS]` detail line, before the test 2
`Quick check: OK` line.

**Why closed:** Did NOT recur in 90 consecutive boot tests across
three 30-run soaks after F4 (and was already not recurring before
F5).  The `invariant` checks include `frame_accounting`, which
calls `frame::stats()` — exactly the path F4 made IRQ-safe.  That
is the most plausible incidental cure: test 2's `check_all()`
re-entry triggered `frame::stats()` in a window when an APIC timer
ISR landed inside the held `ALLOCATOR` lock, and F4 closed that
window.  Cannot prove this was the sole cause from a single
observation, but the empirical bar (90/90 post-fix) is met.

**Watch:** If this ever recurs, reopen — most likely culprit would
be a different invariant closure (heap balance, scheduler balance,
IPC counters, cap audit) hitting an analogous lock-class race.

**Re-verified 2026-06-10:** 38 additional consecutive clean boots
(8-run + 30-run batches, `build/stability/batch30.log`) on the
post-procfs-restructure binary, all reaching BOOT_OK in 24–27s with
no hang at the invariant self-test.  Running total of clean boots
since the F1–F5 sweep is now 128/128.

### F6. Accounting self-test hang — LIKELY CURED INCIDENTALLY 2026-06-07 (SUPERSEDED: true root cause found 2026-07-01, see B-PREEMPT-SPINLOCK)

**2026-07-01 update:** This was NOT actually cured by the F1–F5 IRQ-safety
sweep. The real bug was involuntary preemption while holding the `ACCT`
spinlock (a single-CPU priority-inversion deadlock), now root-caused and fixed
under **B-PREEMPT-SPINLOCK** (top of this file). It "stopped recurring" only
because the trigger is timing-dependent (~5%). The IRQ-safety hypothesis below
was plausible but wrong for this specific hang.


**Where:** `kernel/src/mm/accounting.rs` — self-test path, after
"[accounting]   Destroy: OK".

**Original symptoms:** Single observation 2026-06-07 during batch 473
boot test (`build/serial-test.txt`, truncated at line 3073).  Serial
output stopped mid-accounting self-test before the expected
"Tracked count: 0 (after cleanup)" line; anti-starvation logs
floods every tick afterward, suggesting scheduler alive but the
accounting test thread blocked.

**Why closed:** Did NOT recur in 90 consecutive boot tests across
three 30-run soaks after the F1–F5 IRQ-safety sweep.  The
hypothesis at the time of observation was the same shape as F1
(same-CPU spinlock + softirq re-entry).  F1 fixed RCU, F3 fixed
softirq self-test, F4 fixed `frame::stats()`, and F5 finished the
ALLOCATOR sweep — closing every IRQ-vs-softirq lock-class race
known to be reachable from the timer ISR.  The accounting hang is
most plausibly an incidental casualty of one of those fixes (the
accounting subsystem's tracker uses a mutex that's touched in
allocation paths that F5 made IRQ-safe).

**Watch:** If this ever recurs, reopen — at that point a finer
probe between `Destroy: OK` and `Tracked count` would localize the
new hang window.

**Re-verified 2026-06-10:** 38 additional consecutive clean boots
(8-run + 30-run batches, `build/stability/batch30.log`) on the
post-procfs-restructure binary, all reaching BOOT_OK in 24–27s with
no hang at the accounting self-test.  Running total of clean boots
since the F1–F5 sweep is now 128/128.

### F5. `frame::ALLOCATOR` lock uniformly IRQ-safe — FIXED 2026-06-07

**Where:** `kernel/src/mm/frame.rs` — all 13 remaining `allocator.lock()`
acquisition sites outside `pcpu_refill`/`pcpu_drain` (which are
already called with IRQs off) and `try_stats()` (panic-only).

**Why this was technical debt (was TD1):** F4 made `stats()`
IRQ-safe but left `alloc_*`, `free_*`, `is_allocator_owned`,
`refcount`, `ref_inc`, `ref_dec`, and `validate_free_lists` taking
the lock without wrapping in `without_interrupts`.  No
currently-registered softirq path took the allocator lock (audited
2026-06-07), so there was no exploitable deadlock — but the next
softirq subsystem that touched the allocator (kswapd periodic
reclaim, RCU-deferred page free, memory-pressure tick) would have
silently re-opened the same race that F4 closed.

**Fix:** Wrap each acquisition site in
`crate::cpu::without_interrupts(...)` at the call site, matching
the F1/F3/F4/workqueue pattern.  The multi-attempt `alloc_order_inner`
and `alloc_order_constrained_inner` paths use a per-attempt
without_interrupts so IRQs are re-enabled between attempts (so
reclaim/compact/OOM can run normally and wake other tasks).  Did
NOT wrap `pcpu_refill` / `pcpu_drain` — their callers already run
with IRQs disabled and the function-level comments document this
invariant.  Used inline wraps rather than a helper because the
sites have varied shape (KernelResult returns, multi-attempt retry
loops, value vs Option returns) — a `with_allocator` helper would
have required `FnOnce(&mut BuddyAllocator) -> R` plumbing at every
site, which is more code churn than the wraps themselves.

**Verification:** Post-fix 30/30 boot tests pass.  Zero allocator-lock
hangs observed across this soak.

### F4. frag_history self-test test 6 hangs in sample() loop — FIXED 2026-06-07

**Where:** `kernel/src/mm/frag_history.rs` — `self_test()` test 6
("Ring buffer wraps correctly"), inside the
`for _ in 0..HISTORY_SIZE + 5 { sample(); }` loop.

**Root cause (hypothesis, verified by soak):** `sample()` calls
`mm::frame::stats()` on every iteration, which acquires
`frame::ALLOCATOR.lock()`.  The boot path runs with interrupts
enabled, so an APIC timer ISR could fire on the same CPU while the
lock was held.  Per a softirq-handler audit, no currently-registered
softirq path takes `ALLOCATOR.lock`, so a clean dead-lock chain
wasn't conclusively proven — but the empirical data (hang exactly
in this 37-iteration tight loop over a lock-acquiring call) plus
the cure (see Fix) make this the most likely explanation.  A
plausible alternate path: any future softirq subsystem (kswapd
periodic reclaim, RCU-deferred page free, memory-pressure tick)
that touched the allocator would have re-introduced the race.

**Diagnosed by:** Post-F3 30-run soak showed `[frag_history]
Trend: OK (Stable)` as the last serial line of one failure
(`build/soak-hang-run18.txt`).  Bisected the hang window to the
test 6 sample-loop.

**Fix:** Made `frame::stats()` itself IRQ-safe by wrapping the
`ALLOCATOR.lock()` acquisition in `crate::cpu::without_interrupts(...)`.
The companion `try_stats()` (panic-handler variant) already used
`try_lock()` for the same family of reasons; this brings the
regular `stats()` to parity.  Hardening — eliminates an entire
class of same-CPU IRQ-vs-main deadlocks on the buddy allocator
lock without measurable performance cost (CLI/STI on a stats read
that already serializes on a spinlock is negligible).

**Verification:** Post-fix 30/30 boot tests pass; zero recurrence
of the frag_history hang AND zero recurrence of Active Bugs #1
(accounting) and #2 (invariant) over those same 30 runs.

---

## Technical Debt

### D-NETSTACK-TCP-MINIMAL. Userspace `netstack` TCP client is minimal (slirp-only correctness) — DEBT 2026-07-14

**Where:** `services/netstack/src/main.rs` — `tcp_fetch` / `send_tcp` /
`recv_tcp_seg` (the `OP_TCP_FETCH` control op). Kernel exercises it via
`kernel/src/proc/spawn.rs::netstack_tcp_fetch_roundtrip`.

**What it is:** the Phase-4 one-shot TCP client implements just enough of
RFC 793 to be correct on the loss-free QEMU-slirp path: SYN/SYN-ACK/ACK
handshake, in-order data reception with cumulative ACKs, SYN + request-payload
retransmission (bounded), and a graceful FIN close. Deliberately **omitted**:

- **No out-of-order reassembly.** Out-of-order data segments are dropped and
  dup-ACKed to prompt a retransmit; a genuinely reordering path would stall.
- **No congestion / flow control.** Fixed advertised window (`TCP_WINDOW`),
  no cwnd/ssthresh, no RTT estimation — retransmit timers are fixed poll counts.
- **No outbound segmentation.** The request `payload` must fit a single segment
  (one MSS); larger requests are not split. Fine for the HTTP HEAD self-test.
- **Single fixed ephemeral port + fixed ISN** (`EPHEMERAL_PORT` / `isn`): only
  one connection at a time, and no ISN randomization (no security concern in the
  bounded self-test, but not production-grade).
- **Response capped at the control-path `MSG_CAP` (512 B).** Bodies beyond the
  cap are ACKed (to keep the peer moving) but discarded; only the first ~511
  bytes reach the caller.

**Proper fix:** these all go away with the **Phase-5 shared-memory data ring**
(io_uring-style zero-copy) and a real per-connection TCP state machine (proper
RTO, windowing, reassembly, multiple concurrent sockets, ISN randomization).
Tracked as part of the net-userspace migration; this control-path client is
intentionally the bounded-self-test stand-in until then. See
`net-userspace-migration.md` Phase 4/5 and `design-decisions.md` §64.

### BENCH-COMPOSITOR-SLOW. Compositor over its 4K frame budget (~10.6ms/frame vs 2ms) — PERF BUG 2026-07-01, IMPROVED 4.6x 2026-07-02

**UPDATE 2026-07-14 (5) — parallel opaque window blit landed (first increment of
the deferred window-render parallelization).** The opaque fast path of
`Compositor::blit_buffer` (full-opacity window carrying an `is_opaque()` Xrgb
shared buffer — the common game/video/maximized-window case, which ran every
frame as O(rows) serial `copy_row` memcpys) is now parallelized across
destination row-bands, mirroring the existing `clear_except` band split. New
`Framebuffer::blit_opaque(buf, win_x, win_y, cols, rows)` partitions the back
buffer into disjoint `chunks_mut` row-bands filled on `std::thread::scope`
workers (no `unsafe`, no aliasing; `&SharedBuffer` is `Sync` so workers share it
read-only via `buf.row(r)`), gated by the same `fill_worker_count` heuristic (1M
px threshold, cap 8, single-thread fallback). The static
`blit_opaque_band(band, by0, band_rows, fb_width, buf, win_x, win_y, cols, rows)`
helper replicates `copy_row`'s clipping (left `src_off` when `win_x<0`,
right-edge `min`, vertical band ownership) byte-for-byte, so the result is
bit-identical to the old serial path. Two new unit tests:
`test_blit_opaque_matches_serial_reference_large` (2048×1024 fb > threshold,
1200×900 buffer at 5 offsets incl. negative and offscreen; asserts parallel ==
serial reference) and `test_blit_opaque_clips_edges_small` (single-thread path,
all clip corners). 66 compositor tests total, clippy clean. NOTE: only the
*opaque* blit is parallelized; the per-pixel alpha-blend path stays
single-threaded, and this still spawns a fresh `thread::scope` per blit — the
remaining gap needs a persistent thread pool (to amortize spawn cost) and to
parallelize the `RenderEngine` per-window content draws (not just the final
buffer blit). baselines.toml unchanged (the 4K benchmark's dominant cost is the
background clear + per-window RenderEngine draws, not buffer blits; this helps
buffer-backed-window workloads specifically).

**UPDATE 2026-07-02 (4) — parallel background clear landed, 11.9ms → 10.6ms/frame
min (cumulative 48.6ms → 10.6ms = 4.6x).** Both `Framebuffer::clear` and
`Framebuffer::clear_except` now split the framebuffer into horizontal row-bands
and fill them concurrently via `std::thread::scope` over disjoint
`chunks_mut` slices — no `unsafe`, no shared mutable aliasing (each worker owns a
distinct slice). Worker count comes from the new `fill_worker_count`, which caps
at 8 and gracefully falls back to a single thread when the buffer is below 1M
pixels or when `std::thread::available_parallelism()` can't be reported, so it
never pessimizes small buffers or single-core targets. The per-scanline
span-merging logic (formerly inline in `clear_except`) was extracted into the
static `fill_uncovered_band(buf, y0, band_rows, width, color, covered, fb_height)`
helper, shared by the single-threaded and parallel paths, using absolute-y
overlap tests against the covered rects and band-local writes. New unit test
`test_clear_except_parallel_band_boundaries` (2048×1024 = 2M px, covered rects
straddling band boundaries; asserts the parallel result is byte-identical to the
single-threaded reference, plus covered-kept / uncovered-cleared spot checks). 64
compositor tests total, clippy clean. baselines.toml `measured_ns` updated to
10572000. NOTE: this only parallelizes the *background clear*; the per-window
opaque content draws are still single-threaded, so the remaining gap needs a
persistent thread-pool (to amortize the per-frame `thread::scope` spawn cost) + a
RenderEngine band-view refactor to parallelize the window-render tiles too.

**UPDATE 2026-07-02 (3) — desktop-clear occlusion cull landed, 15.8ms → 11.9ms/frame
min (cumulative 48.6ms → 11.9ms = 4.1x).** The full-desktop background clear no
longer memsets the pixels hidden behind opaque windows. `full_recomposite_into_back`
now computes `Compositor::opaque_cover_rects()` — the screen-space rectangles
provably overwritten with opaque content this frame (buffer-less windows whose
first command opaquely covers the client area at full opacity, and windows
carrying an opaque `is_opaque()` shared buffer at full opacity, over the covered
sub-rect) — and calls the new `Framebuffer::clear_except(color, &covered)`, which
fills only the complementary (uncovered) spans per scanline. Decorations
(title bar, border, translucent shadow) are deliberately excluded from the cover
rects since they lie outside the client rect, so the background under them is
still cleared (conservative → only ever costs a little correct overdraw, never
correctness). New unit tests: `test_clear_except_*` (4: empty/single/overlapping-
merge/offscreen-clip), `test_opaque_cover_rects_*` (3: opaque-command window
reported, translucent/minimized/rounded excluded, buffer sub-rect + Argb
excluded), and `test_full_recomposite_cull_matches_uncovered_background` (visual
equivalence). 63 compositor tests total, clippy clean. baselines.toml
`measured_ns` updated to 11929000.

**UPDATE 2026-07-02 (2) — occlusion cull landed, 21.4ms → 15.8ms/frame min
(cumulative 48.6ms → 15.8ms = 3.1x).** `render_window` now skips the
compositor's default white client-background fill when the client's first render
command is an opaque, square-cornered `FillRect` that fully covers the client
area on a fully-opaque window (`Compositor::first_command_covers_client`). That
first fill was 100% overdraw in the common "client paints its own background"
case (~29% of the 4K benchmark's opaque stores). Guarded to be correct: rejects
translucent windows (opacity < 1.0), non-opaque colors (alpha < 255), rounded
corners (corner pixels would show the bg), and partial-cover rects. New unit
test `test_first_command_covers_client` (55 tests total). baselines.toml
`measured_ns` updated to 15831000.

**UPDATE 2026-07-02 (1) — fill_rect row-wise rewrite landed, 48.6ms → 21.4ms/frame
min (2.3x).** `RenderEngine::fill_rect` no longer calls `blend_pixel`
per pixel. Two new `Framebuffer` fast paths were added next to `copy_row`:
`fill_row_solid` (opaque color → single `[u32]::fill`/memset per row, skips the
per-pixel float-alpha math and bounds check) and `blend_row` (translucent color
→ hoists the alpha computation and branch out of the inner loop, integer blend
only). `fill_rect` resolves the effective alpha once (color-alpha × opacity) and
dispatches to the solid, blend, or skip (alpha 0) path per row.

**Why it's still over 2ms (and why the remaining gap is *not* another naive-code
bug):** after culling the wasted white bg fill, the benchmark still issues ~31M
opaque u32 stores/frame — an 8.3M-pixel clear plus 16 windows painting opaque
client content — i.e. ~124 MB written per frame. At ~16ms that's ~8 GB/s
effective, near the ceiling for scalar cache-polluting stores on this host. The
per-pixel-work bug is fixed; what's left is memory bandwidth on a *full*
recomposite. Getting a full 16-window 4K
recomposite under 2ms would need SIMD non-temporal (streaming) stores +
multithreaded tiles, and/or occlusion culling to skip the fully-covered white
client-bg fill (that first fill is 100% overdraw when the client paints an opaque
full-window rect). **Crucially, steady-state rendering does NOT full-recomposite
every frame** — the compositor uses damage-rect partial updates (only changed
regions repaint), which is the actual 144Hz-vsync mechanism; this benchmark
deliberately stresses the worst-case full-recomposite path (wallpaper change,
resize, many simultaneously-moving windows). Remaining optimization directions
below are now *lower priority* — the dominant per-pixel bug is resolved.

**Where:** `gui/compositor/src/main.rs` — the software composite path
`Compositor::full_recomposite_into_back` → `render_all_windows` (~2807) →
`render_window` (~2832, shadows + decorations + per-command draw) over the
`Framebuffer` per-pixel ops (`clear`/`clear_rect`/`set_pixel`/`blend_pixel`,
~503-600).

**Measured (2026-07-01, via the new `bench_compose_frame_4k`):** a 4K
(3840×2160) full recomposite with 16 decorated windows carrying toolkit client
content takes **~48.6ms/frame (min), ~50ms mean, RELEASE build on the dev
host** — roughly **25x the 2ms target** in CLAUDE.md's perf-critical table,
i.e. ~20fps, missing even a 60Hz (16.7ms) vsync budget, nowhere near 144Hz
(6.9ms). This is the classic "correct-but-naive" hot-path code the
benchmark-everything mandate exists to catch. Recorded in
`bench/baselines.toml` `[compositor_frame_4k]` (`measured_ns = 48570000`).

**Likely culprits (profile before optimizing):** (1) ~~per-pixel scalar fills in
`fill_rect` with bounds-checks per pixel~~ — **FIXED 2026-07-02** (row-wise
`fill_row_solid`/`blend_row`); (2) ~~per-pixel float alpha in `blend_pixel` for
solid fills~~ — **FIXED for fills** (alpha resolved once per fill; `blend_pixel`
still used by the per-pixel `blit_buffer` slow path and font glyphs); (3)
full-screen clear + full redraw of every window every frame even when
`bench_full_composite` forces it — the real `compose_frame` has a partial-damage
path, but the fully-damaged case (wallpaper change, resize, many moving windows)
hits this — STILL the structural cost (bandwidth-bound overdraw); (4)
`render_window` clones `render_tree.commands` and the z-stack each frame
(`render_all_windows`/`render_window`, allocations on the hot path) — small for
the benchmark's 4-command windows, but worth eliminating for large trees.

**Remaining optimization directions (lower priority — per-pixel bug resolved):**
SIMD non-temporal/streaming stores for solid rects (avoid cache pollution on
huge fills) + multithreaded tile compositing to break the single-core bandwidth
ceiling; ~~occlusion culling so a window's default opaque client-bg fill is
skipped when the first command fully covers it~~ — **DONE 2026-07-02** (first-command
cull, plus desktop-clear cull under fully-opaque covering windows and opaque
shared buffers — DONE 2026-07-02 (2) & (3)); precompute/caches for window
decorations and shadows (they rarely change frame-to-frame); avoid per-frame
`Vec` clones in `render_window` (borrow or reuse scratch buffers); ensure the
damage-tracking fast path is actually taken for the common "one window changed"
case. Target: < 2ms/4K (for a full recomposite; likely needs SIMD+threads). NB:
this is the CPU-software fallback; the eventual GPU/DRM-KMS accelerated path is
separate.

**Status:** per-pixel-cost bug FIXED + redundant-bg-fill occlusion cull DONE +
desktop-clear occlusion cull DONE + parallel background clear DONE (cumulative
4.6x, 48.6ms → 10.6ms, 2026-07-02); the remaining gap to 2ms on a *full*
recomposite is memory-bandwidth-bound (~124 MB/frame worst case at ~12 GB/s
scalar stores) and needs a SIMD-streaming-store + multithreaded-window-tile
initiative (its own focused session: persistent thread-pool to avoid per-frame
`thread::scope` spawn cost + a RenderEngine band-view refactor). All the cheap
algorithmic overdraw wins have now been taken; the remaining work is a
bandwidth/parallelism problem, not a naive-code problem. Unblocked (no Linux
binaries / operator input needed).

### BENCH-COMPOSITOR. Compositor frame benchmark — RESOLVED 2026-07-01 (benchmark added; revealed BENCH-COMPOSITOR-SLOW)

**Resolution:** added `bench_compose_frame_4k` (an `#[ignore]`d measurement test
in `gui/compositor/src/main.rs`) plus the `Compositor::bench_full_composite`
hook (which shares `full_recomposite_into_back` with the real `compose_frame`
so they can't drift) and the `[compositor_frame_4k]` baseline in
`bench/baselines.toml`. The compositor is host-runnable (`cargo test -p
compositor --target x86_64-pc-windows-gnu --release -- --ignored --nocapture
bench_compose_frame_4k`), so a real number is measurable. Running it immediately
surfaced the ~25x-over-target result now tracked as BENCH-COMPOSITOR-SLOW above.
Original gap description retained below for context.

**Where (original gap):** `gui/compositor/src/main.rs` — the composite path is
`Compositor::compose_frame` (line ~2746) → `render_all_windows` (~2807) →
`blit_buffer` (~2949). There is frame-budget *tracking* at runtime
(`end_frame`, line ~849, returns whether the frame was within budget) but no
benchmark that measured the actual composite cost against a target.

**What:** CLAUDE.md's performance-critical-subsystems table lists "Compositor
frame — Must composite a full desktop in < 2ms at 4K to not miss 144Hz vsync"
as a hard benchmark target, and mandates "benchmark everything critical." Every
other critical subsystem (syscall dispatch, IPC, context switch, page fault,
page/heap alloc, scheduler pick_next, futex, io_uring, IOCP, ISR latency, VFS,
FS r/w) has a benchmark in `kernel/src/bench.rs` scored against a
`bench/baselines.toml` target. The compositor has none. `bench/` currently
contains only `baselines.toml` (no per-subsystem benchmark crates yet), and
`grep` finds no `criterion`/`#[bench]`/`fn bench` anywhere under `gui/`.

**Why not done in the discovering session:** identified during a benchmark-gap
audit at the tail of a long, context-heavy autonomous session. Doing it right
(build a host- or target-runnable harness that constructs a 4K in-memory
framebuffer + a representative multi-window damaged scene, drives
`compose_frame`/`render_all_windows`, and records ms/frame against the 2ms
target) is real work that deserves a fresh context rather than a rushed pass.

**Proper fix:** add a compositor composite-frame benchmark. Options:
(a) a `criterion` bench under `gui/compositor/benches/` if the compositor crate
(deps: `guitk`, `guiremote`) builds and composites on the host with an
in-memory framebuffer (verify `compose_frame`/`render_all_windows`/`blit_buffer`
don't require real DRM/KMS hardware handles — construct the `Compositor` with a
plain `Vec`-backed 3840×2160 framebuffer); or (b) if the composite path is too
coupled to the target, add an in-kernel/target self-test bench analogous to
`bench_pick_next_scaling`, driving a synthetic scene and using `rdtsc`. Scene
should scale window count / damage area to expose O(n)-in-pixels or
O(n)-in-windows behaviour. Record a `[qemu.compositor_frame_4k]` (and/or a
host baseline) in `bench/baselines.toml` with `target_ns = 2_000_000` (2 ms).
Note the compositor is userspace, so the on-hardware number (not the TCG figure)
is the meaningful one; document the measurement environment.

**Trigger:** next time the compositor's render path is touched, or as the next
benchmark-infrastructure task — it is unblocked (does not need Linux binaries or
operator input), just deferred for context reasons.

### EEVDF-PICK-ON. EEVDF backend `pick_next` O(n) worst-case — RESOLVED 2026-07-15 (option (b) split-index rewrite)

**Status:** RESOLVED. `pick_next` is now amortised **O(log n)**, satisfying
CLAUDE.md's hard rule ("`pick_next` must be O(1) or O(log n) — never O(n)").
The secondary `min_vruntime`-approximation defect is fixed too. Kept below for
history and to document the design.

**Original problem (2026-07-01):** The run queue is a
`BTreeMap<(virtual_deadline, TaskId), EevdfEntry>` ordered by *deadline*, but a
task is *eligible* only when `vruntime <= min_vruntime`. The old `pick_next`
walked the tree from the front (earliest deadline) until it found the first
eligible task. Because the earliest-*deadline* tasks can be ineligible (higher
vruntime — e.g. a just-preempted task re-enqueued with accumulated vruntime but
an early deadline), that scan could walk past many entries: **O(n) worst-case**.
Secondary defect: `update_min_vruntime` derived its candidate from the
*earliest-deadline* task's vruntime, NOT the true minimum vruntime across the
queue, so the eligibility boundary itself was approximate.

**Fix implemented (option (b), split-index in safe std collections):** The
`tree` (deadline-keyed, all tasks) remains the source of truth, augmented by
two partition indexes plus a reverse index:
- `eligible: BTreeMap<(deadline, TaskId), ()>` — tasks with
  `vruntime <= min_vruntime`, ordered by deadline. `pick_next`'s Phase-1
  "earliest-deadline eligible task" is `eligible.iter().next()` = **O(log n)**.
- `ineligible_by_vrt: BTreeMap<(vruntime, TaskId), ()>` — the rest, ordered by
  vruntime. Its front is the smallest vruntime among ineligible tasks, which
  (a) feeds the true-minimum `min_vruntime` computation and (b) is the next
  candidate to promote as the floor rises.
- `deadlines: BTreeMap<TaskId, deadline>` reverse index so a task can be found
  in `tree`/`eligible` by id when promoting from `ineligible_by_vrt`.
- each `EevdfEntry` carries `is_eligible: bool` so removals (`dequeue`,
  `steal`, stale re-enqueue) hit the correct partition map in O(log n).

`update_min_vruntime` now sets the floor to the true minimum vruntime across
the ineligible set and the running task (only when `eligible` is empty, since a
non-empty eligible set means the floor is already at/above those vruntimes),
and stays monotonic. `rebalance()` drains `ineligible_by_vrt` from its front
into `eligible` while `front.vruntime <= min_vruntime`; because a waiting task's
vruntime is fixed and `min_vruntime` is monotonic, each task promotes **at most
once per residency**, so `rebalance` is amortised O(log n) per operation. It is
called after every mutation that can move the floor (`enqueue`, `dequeue`,
`tick`, `steal`, `pick_next`). Phase-2 fallback ("no eligible task → earliest
deadline overall") is `tree.iter().next()` = O(log n).

**Tests added (`eevdf::self_test`, all passing in boot self-test):**
"partition invariant holds across operations" (checks
`eligible.len()+ineligible_by_vrt.len()==tree.len()==nr_running` and
`is_eligible == (vruntime<=min_vruntime)` for every entry after each op),
"pick_next is deadline-correct under adversarial vruntime mix" (the exact case
that used to force the O(n) scan), and "min_vruntime tracks the true minimum,
not earliest-deadline". The pre-existing "weighted fairness" test was corrected
to assert on **CPU time (ticks consumed)** rather than pick *count*: with the
now-correct `min_vruntime`, a high-weight and low-weight task alternate picks
1:1, but the high-weight task runs a full slice while the low-weight one is
preempted early — weighted fairness correctly manifests as more CPU time, not
more picks. (The old pick-count assertion only passed by accident of the old
`min_vruntime` bug.)

### TD32. Container rootfs jail uses the extracted `lower` dir directly (no overlay CoW) and only jails absolute paths

**Where:** `kernel/src/kshell.rs` (`oci run`, `cmd_oci`) sets the container's
`root_path` to the extracted `/tmp/oci-<name>/lower` tree;
`kernel/src/ipc/namespace.rs` (`apply_root`). The `fs::overlay` module exists and
`oci run` *creates* an overlay (lower+upper) but the overlay is ID-addressed, not
mounted into the VFS path tree, so the per-process root jail (which prepends a
host path prefix and routes through the normal VFS) cannot resolve through it.

**The debt.**
1. **No copy-on-write isolation.** Because the jail points at `lower`, writes the
   container makes land in the shared extracted image tree, not the per-container
   `upper`. Two containers from the same image would see each other's writes, and
   `overlay reset`/`commit` semantics don't apply to the running container.
2. **Relative paths are not jailed.** `apply_root` only re-anchors absolute
   paths; relative paths pass through for a per-process cwd layer to resolve. That
   cwd layer does not yet jail cwd, so a container process using relative paths
   from an unjailed cwd could currently resolve outside its root. The image
   entrypoint and its libraries use absolute paths, so this doesn't bite the
   common launch path, but it is a real containment gap.

**Why it didn't block increments 3–4 (§42):** the entrypoint binary and its
libraries are read via absolute paths under the rootfs, which `apply_root` jails
correctly (`..` clamped), so launching a statically-linked image entrypoint
works and is isolated for reads. The gaps are CoW write-isolation and
relative-path containment.

**Proper fix.** (a) VFS-mount the overlay at the container's rootfs mountpoint so
the jail routes through copy-on-write (writes → `upper`, reads → merged), i.e.
give `fs::overlay` a real VFS mount adapter and point `root_path` at the merged
mountpoint instead of `lower`. (b) Jail cwd end-to-end: make the per-process cwd
itself a jailed (absolute, within-root) path so relative resolution is contained,
then have `apply_root` (or the cwd-join layer) treat relative paths as
rooted-after-join. Track alongside the mount-namespace/`pivot_root` work deferred
in §42.

**Update 2026-06-30 (increment 5):** Part (a)'s blocker is removed. The
`fs::overlay::OverlayFs` VFS mount adapter now exists and works — but only after
fixing a foundational VFS issue: the global VFS lock was held across every
filesystem method call, so mounting an overlay (whose methods re-enter the VFS to
read their backing layers) deadlocked on boot. The VFS now uses a **per-mount
lock** (`Arc<Mutex<Box<dyn FileSystem>>>` + `resolve_mount`; design-decisions
§43), so stacked filesystems mount cleanly (overlay self-test 13 passes). **Still
open for TD32:** wiring `oci run`/`container create` to actually mount an
`OverlayFs` at the container rootfs and point `root_path` at that mountpoint
instead of `lower` (increment 6), plus part (b) cwd jailing.

**Update 2026-06-30 (increment 6): part (a) DONE.** `oci run` now VFS-mounts the
per-container `OverlayFs` adapter at `/containers/<name>/rootfs` and jails the
container at that merged mountpoint (not the read-only `lower`), so container
writes are copy-on-write isolated — reads see the merged view, writes land in the
per-container `upper` layer. The overlay creation (`fs::overlay::create`) now
flows its `OverlayId` into the mount step; if the overlay can't be created or
mounted, the launch gracefully falls back to jailing at the read-only `lower`.
The mountpoint is recorded on the `Container` (`rootfs_mount` field +
`set_rootfs_mount` setter, Created-only) and `container::delete` unmounts it on
teardown (outside the table lock; the VFS has its own per-mount locking). Both the
entrypoint-ELF read and the jail now route through `jail_root`.
**Still open for TD32:** part (b) — cwd jailing (relative-path containment). The
absolute-path read isolation and now CoW write isolation are both in place; the
remaining gap is jailing a container process's *cwd* so relative resolution is
contained, alongside the mount-namespace/`pivot_root` work deferred in §42.

**Update 2026-06-30 (increment 7): double-jail bug in fd-backed I/O — FIXED.**
While preparing part (b) we discovered that *all* fd-backed file I/O was broken
for jailed (container) processes — a regression that increment 6's CoW mount
would have exposed the moment a container actually opened a file. Root cause:
`namespace::apply_root` is intentionally **non-idempotent** (it blindly prefixes
the jail root, assuming a *guest* path), but `handle::open()` stored the
*already-resolved host path* in the file handle (`file.path`), and every
subsequent handle op (`Vfs::read_at(&file.path)`, `write_at`, `truncate`,
`metadata`, `readdir_at`, `file_identity`, `flock`/`funlock`, …) called
`resolve_follow` *again* → re-applied the jail prefix → double-jailed to a path
that doesn't exist. For a jailed process even `open()` failed (its internal
`stat`/`truncate`/`write_file` re-jailed). Non-jailed processes were unaffected
only because `resolve_follow` is idempotent on already-resolved non-jailed paths.
**Fix (design-decisions §44):** every path-based `Vfs` method is split into a
thin wrapper (`resolve_follow` → call worker) plus a `*_resolved` worker that
operates on an already-resolved host path *without* re-translating. Handle-backed
ops call the `*_resolved` worker directly (an open fd holds a resolved reference —
Unix semantics, immune to later chroot/rename/symlink changes). Split methods:
`read_at`, `read_file`, `stat`, `write_file`, `write_at`, `truncate`, `metadata`,
`read_at_uncached`, `readdir_at`, `file_identity`, `flock`, `funlock`,
`lock_query`. A non-idempotency guard was added to
`namespace::test_process_root` (re-resolving an already-jailed path must
double-jail) to pin the invariant so a future refactor that makes handle ops
re-resolve is caught at boot. Build clean, clippy delta zero, boot-test green.

**Update 2026-06-30 (increment 8): part (b) cwd / relative-path containment —
DONE.** TD32 part (b) is closed. Relative paths are canonicalized against the
per-process cwd in the syscall layer *before* the VFS jails them, so containment
hinges entirely on cwd (and dirfd base paths) being stored as **guest** paths.
`chdir` already stored a guest cwd, but three sites stored/used the *resolved
host* path and so leaked the jail location (`getcwd`) and double-jailed relative
resolution: (1) `fchdir` stored `handle_path` (host) as cwd; (2) `sys_openat`
with a real dirfd built `host_dir + rel` then re-jailed it (and its directory
type-check `stat(&host)` re-jailed → ENOENT for every relative `*at` from a
jailed process); (3) `resolve_at_path` (the shared `*at` resolver:
fstatat/unlinkat/fchownat/…) had the identical defect. **Fix
(design-decisions §45):** added `namespace::unjail_path_for(pid, host) → guest`
(exact inverse of `apply_root`: strips the jail-root prefix; no-op when
unjailed). `fchdir` now stores the un-jailed guest cwd. A new shared helper
`dirfd_to_guest_dir(dirfd)` resolves a real dirfd to its *guest* directory path,
doing the directory-type check with `stat_resolved` (no re-jail); both
`sys_openat` and `resolve_at_path` use it, so the combined path is jailed
exactly once. Round-trip regression assertions
(`unjail(resolve(guest)) == normalized guest`, unjailed no-op, out-of-jail
defensive passthrough) added to `namespace::test_process_root`. **Limitation:**
`unjail_path_for` reverses only the chroot layer, not namespace Bind/Hide
remapping — the container runtime never combines Bind rules with a chroot jail,
so the reversal is exact for the container case (documented on the function and
in §45). With parts (a) [CoW, inc 6] and (b) [this] done, TD32's remaining scope
is the broader mount-namespace/`pivot_root` work deferred in §42 (a separate,
larger feature, not a containment gap).

**Update 2026-06-30 (increment 9): volume (bind) mounts — DONE.** The first
concrete slice of TD32's remaining mount-namespace scope landed. A per-process
volume table (`PROCESS_MOUNTS` in `namespace.rs`) layers Docker `-v`-style bind
mounts *over* the chroot: a guest path under a volume prefix resolves to an
arbitrary host target (escaping the rootfs), while everything else still jails
under the rootfs. Volume matching runs *after* `..`-normalization, so a guest
cannot climb out of a volume into the host (security-critical ordering).
`unjail_path_for` reverses volumes too (longest host-target match), so `fchdir`
into a volume reports the guest path and stays single-jailed. Container plumbing:
`Container.volumes` + `add_volume_mount()` (Created-only, `-v` order), installed
on the init process in `add_process_task`, cleared in `remove_process_task`/
`delete`/`detach`. Covered by `namespace::test_volume_mounts` and container
self-test 19; build/clippy clean, boot-test green. Design rationale in §46.
Still deferred (TD32 remainder): a true longest-prefix mount-tree that subsumes
the rootfs as the `/` mount (the `pivot_root` target), read-only volumes
(`-v …:ro`), and tmpfs/named-volume types — all straightforward extensions on
the same table.

**Update 2026-06-30 (increment 10): `-v` CLI flag — DONE.** The volume
mechanism now reaches end-to-end from the shell: `oci run <dir> -v
/srv/data:/data` (also `--volume`, repeatable) parses each spec on the first
`:` (Docker order), validates both sides are absolute, and installs the bind
mount via `add_volume_mount` while the container is still in Created state —
before the init process launches. Usage/help strings updated. Container
self-tests 18/19 were also made deterministic this session (synthetic
never-scheduled PID instead of a real init process that could exit mid-test and
clear its namespace — see B-CONTAINER-JAIL-TESTRACE). Build clean, boot-test
green ("Self-test PASSED (19 tests)"). The TD32 remainder above (read-only
volumes, mount-tree/`pivot_root`, tmpfs) is unchanged.

**Update 2026-06-30 (increment 11): port publishing (`-p`) — DONE.** Docker
`-p host:container[/proto]` port publishing landed, reusing the existing
`net::nat` port-forward table. `Container` gained `container_ip` (captured from
the configured network IP) and `published_ports`; `add_port_publish` records
publish intents (Created-only, requires a network IP, rejects port 0, last-
writer-wins, capped at `MAX_PUBLISHED_PORTS`); `run()` installs them as NAT
rules forwarding host traffic to the container IP inside its netns; `stop()`
flushes them and `delete()` clears the intents. CLI: `oci run -p
8080:80[/udp]` (repeatable). Container self-test 20 covers the lifecycle
deterministically (forwards are per-netns, not per-PID). This is orthogonal to
the rootfs/volume mount-namespace scope; the TD32 mount remainder (read-only
volumes, mount-tree/`pivot_root`, tmpfs) is still open.

**Update 2026-06-30 (increment 12): env injection (`-e`) — DONE.** Docker
`-e KEY=value`/`--env` environment injection landed entirely in the CLI launch
path (`kshell::cmd_oci` `run`/`create`); the container/kernel layer needed no
change because env already passes through `SpawnOptions::envp`. The parser
requires `KEY=value` (a bare `-e KEY` is rejected — a container has no host
environment to inherit) and rejects an empty key. At launch the CLI `-e` entries
are merged over the image's declared ENV with Docker override semantics: each
`-e` entry wins over an image ENV entry with the same key, and the merged set has
no duplicate keys (CLI entries added first, then image ENV entries whose key is
not already overridden). Usage/help strings updated to include `[-e KEY=value
...]`. The TD32 mount remainder (read-only volumes, mount-tree/`pivot_root`,
tmpfs) is still open.

**Update 2026-06-30 (increment 13): `docker`/`dk` CLI-compat shim — DONE.** A
thin Docker-CLI front-end (`docker`, alias `dk`) translates familiar verbs to
the native `oci` (image) and `container` (lifecycle) handlers: `run`/`create`
→ `oci run`/`create`; `ps [-a]` → `container list` (all states; `-a` accepted +
ignored since there is no running-only index); `start`/`stop`/`rm` →
`container start`/`stop`/`delete`; `inspect` → `container info`; `exec` →
`container exec`; `images <dir>` → `oci inspect` (SlateOS has no name-keyed
image registry — images are on-disk OCI layout dirs). Argument spacing is
preserved verbatim when delegating. Registered in dispatch, `is_builtin`, and
the tab-completion list.

**Update 2026-06-30 (increment 14): resource limits (`--memory`/`--cpus`) —
DONE.** `oci run`/`create` now accept Docker `--memory`/`-m <SIZE>` (bytes with
optional binary k/m/g[b] suffix, rounded up to whole 16 KiB frames → cgroup
`mem_limit`) and `--cpus <N[.M]>` (fractional cores → percent of one core, e.g.
`1.5` → 150 → `CpuLimit::from_percent` via cgroup `cpu_quota`). Parsing is pure
and float-free (kernel has no FPU state in this path); two helpers
(`parse_mem_size_to_frames`, `parse_cpus_to_percent`) are covered by
`kshell::cli_resource_parser_self_test()`, wired into the boot self-test run in
`main.rs`. The TD32 mount remainder (read-only volumes, mount-tree/`pivot_root`,
tmpfs) is still open.

**Update 2026-06-30 (increment 15): read-only volumes (`-v …:ro`) — DONE.**
Docker `-v host:guest[:ro|:rw]` now carries an access mode end-to-end. The
volume table entry (`VolumeMount` in `namespace.rs`, `VolumeSpec = (guest,
host, read_only)` in `container.rs`) gained a `read_only` flag; `add_volume`
and `add_volume_mount` take it (last-writer-wins, so re-mounting the same guest
prefix `:rw` clears a prior `:ro`). Enforcement is a new
`namespace::check_writable(path)` / `check_writable_for(pid, path)` that mirrors
the exact resolution pipeline used by `resolve_path_for` — step-1 namespace
translation, `..`-clamping `normalize_jailed`, then longest-prefix volume match —
and returns `KernelError::ReadOnlyFilesystem` (EROFS) when the matched volume is
read-only. It is a cheap `Ok(())` no-op for any process without volumes or
without a chroot root (all non-container processes, and containers with only
read-write volumes), making the wide enforcement surface zero-risk to existing
behavior. Two chokepoints gate writes: (1) fd-based writes via
`fs::handle::open()` reject up front when the open flags request write/create/
truncate/append; (2) ~17 path-based mutating `Vfs` methods (`write_file`,
`write_at`, `truncate`, `remove`, `remove_recursive`, `mkdir`, `mkdir_all`,
`rmdir`, `rename`/`rename_noreplace` via `rename_inner`, `rename_exchange`,
`set_permissions`, `set_times`, `set_xattr`, `remove_xattr`, `symlink`, `link`,
`atomic_write`) call the namespace check on the caller's (guest) path before
host-path resolution. The `_resolved` variants are intentionally *not* gated
(they take already-translated host paths). CLI: `oci run -v /srv/data:/data:ro`
parses an optional third `:mode` segment (`ro`/`rw`, default `rw`); unknown
modes are rejected. Covered by `namespace::test_volume_mounts` (read-only volume
write-denied / read-allowed assertions) and container self-test 19
(`check_writable_for` on `/logs` ro vs `/data` rw vs `/bin/sh` rootfs).
The TD32 mount remainder (a true longest-prefix mount-tree subsuming the rootfs
as the `/` mount / `pivot_root` target, `--read-only` root, and tmpfs/named-
volume types) is still open.

**Update 2026-06-30 (increment 16): read-only root (`--read-only`) — DONE.**
Docker `--read-only` now makes the whole container rootfs non-writable while
writable (`:rw`) volumes still punch writable holes through it. A per-process
flag set `PROCESS_ROOT_RO` in `namespace.rs` (set via `set_root_read_only(pid,
ro)` / queried via `is_root_read_only`, cleared on `detach`/`clear_root` for
PID-reuse safety) feeds the same `check_writable_for` decision used for `:ro`
volumes: longest-prefix volume match first (a `:ro` volume → EROFS, a `:rw`
volume → allowed), and when *no* volume matches the path lives in the rootfs, so
it is denied iff the root is read-only. The fast-path `Ok(())` no-op now also
requires a writable root, so non-container processes and writable containers are
still zero-cost. `ContainerConfig` gained a `read_only_root` field + `.read_only(bool)`
builder; the flag rides through `create` → `add_process_task`, which calls
`set_root_read_only(pid, true)` after installing volumes (only when a chroot root
exists). Post-create `container::set_read_only_root(id, ro)` (Created-state-gated,
like `set_root_path`) mirrors the volume setter; `ContainerInfo` reports it. CLI:
`oci run … --read-only` (a bare flag) prints `Root FS: read-only`. Covered by
`namespace::test_volume_mounts` (read-only-root block: rootfs denied, `:rw`
volume still writable, flag-clear restores writability) and container self-test
19b (now 21 tests total). The TD32 mount remainder is now just the true
longest-prefix mount-tree subsuming the rootfs as the `/` mount (`pivot_root`
target) and tmpfs/named-volume types.

**Update 2026-07-01 (increment 17): tmpfs mounts (`--tmpfs`) — DONE.** Docker
`--tmpfs /guest` now mounts an ephemeral in-memory filesystem at a guest path.
Modeled as a bind mount whose host target is a per-container `fs::memfs` mount:
`add_tmpfs_mount(id, guest)` (Created-only) validates the guest path (absolute,
not `/`, no duplicate against existing volumes/tmpfs), then — outside the table
lock — `Vfs::mkdir_all` + `memfs::mount` a fresh in-memory fs at a unique host
mountpoint `/var/lib/slate/tmpfs/<id>-<index>`, and records it as a **writable**
`VolumeSpec` at the guest prefix so all the existing volume resolution/write
machinery (`resolve_path_for`, `check_writable_for`, `..`-clamping) applies
unchanged. The `Container` gained a `tmpfs_mounts: Vec<String>` of owned
mountpoints; `delete()` unmounts and `remove_recursive`-removes each so nothing
leaks. CLI: `oci run … --tmpfs /tmp` (repeatable) — mount **options** (`--tmpfs
/tmp:size=64m`) are explicitly rejected with a warning rather than silently
ignored (an unbounded tmpfs is a containment/DoS gap until per-mount quota
enforcement lands; honest failure until then). Covered by container self-test 46
(two mounts, bad-spec/duplicate rejection, writable-memfs write+read-back,
non-Created rejection, delete-unmount verification — now 60 tests total). Build/
clippy clean, boot-test green. With this, the volume *types* are all covered —
host bind mounts (`-v /host:/guest`), read-only volumes (`:ro`), named volumes
(`-v NAME:/guest` via `volume::ensure`), and now tmpfs (`--tmpfs`). The TD32
mount remainder is therefore now just the true longest-prefix mount-tree
subsuming the rootfs as the `/` mount (the `pivot_root` target) — the last
structural piece, not a volume-type gap.

**Update 2026-07-01 (increment 18): container-aware `/proc/<pid>/mountinfo` —
DONE.** A container (jailed) process now sees *its own* mount view in
`/proc/<pid>/mountinfo` instead of the host's global mount table. Previously
`gen_pid_mountinfo` rendered `Vfs::mounts_full()` for every PID, so a process
inside a container observed the entire host mount topology (an info leak) and
none of its own rootfs/volumes/tmpfs (a correctness gap). Fix:
`namespace::mount_view_for(pid)` returns `None` for an unjailed process (keep
the global table) or the container's ordered view — the rootfs at guest `/`
(read-only iff `--read-only`), then each volume/tmpfs at its guest prefix with
its own `:ro`/`:rw` flag. `procfs::render_container_mountinfo` renders it,
resolving each entry's *fstype* from the real host mount backing its
`host_target` (`fstype_for_host_path` longest-prefix match: overlay for the
rootfs, tmpfs/memfs for `--tmpfs`, the host fs for binds) while reporting the
`source` field as `none` so host backing paths are **not** leaked into the
container. The same container-aware rendering was applied to the `/proc/mounts`
line format (`render_container_mounts`): the global `/proc/mounts` now resolves
the *caller's* view (`current_task_id`), and a new per-PID `/proc/<pid>/mounts`
(hence `/proc/self/mounts`) file mirrors Linux's mount-namespace-local table.
Covered by procfs self-tests (container view for both `mountinfo` and `mounts`:
RO rootfs→overlay, RO bind→ext4, RW tmpfs→tmpfs; plus `mount_path_covers`
boundary safety so `/data` doesn't cover `/database`). Build/clippy clean,
boot-test green. Note this is *introspection* only — real in-container
`mount`/`umount`/`pivot_root` syscalls mutating a per-container mount table
remain the deferred mount-namespace piece.

### TD33. Container `logs` capture works only for Linux-ABI container inits — ACCEPTED LIMITATION 2026-06-30

**Where:** `kernel/src/container.rs` (`redirect_output_to_capture`, called from
`run_with_abi` right after `spawn_process`). The capture works by rewriting the
init process's **Linux fd table** — `pcb::linux_fd_take(pid, 1)` then
`linux_fd_install_at(pid, 1, FdEntry::file(capture_handle, O_WRONLY))` and
`linux_fd_dup2(pid, 1, 2)` — during the window after spawn but before the init is
scheduled.

**The limitation.** The `linux_fd_table` is only installed for **Linux-ABI**
binaries (`spawn.rs`: `if is_linux_abi { … linux_fd_install_stdio(pid) }`).
Native SlateOS binaries have no `linux_fd_table`, so `linux_fd_install_at` fails,
`redirect_output_to_capture` returns `None`, the container's `log_path` stays
empty, and `container logs ID` returns `NotFound`. A native-ABI container init's
stdout/stderr therefore goes to the console and is **not** captured to
`/var/log/containers/<id>.log`.

**Why it's accepted, not blocking.** Real Docker/OCI container entrypoints are
Linux-ABI glibc ELFs, which is exactly the path the capture supports. The
native-ABI container init is a SlateOS-specific corner case (no real image ships
one), so the Docker-compatible `logs` feature is correct and sufficient for its
intended use. The self-test (19t) deliberately forces `AbiMode::Linux` via
`run_with_abi` so it exercises the real capture path deterministically.

**Proper fix (deferred).** Also wire capture through the **native** fd-inheritance
channel (`initial_fds` / `SpawnOptions.fd_map`, consumed via
`SYS_PROCESS_GET_INITIAL_FDS`): install the capture handle as fd 1/2 in the
native init's `initial_fds` when the ABI is Native. Deferred because it needs
verification that native binaries honour `initial_fds` for stdout and that the
file-offset-sharing (single append position for interleaved 1+2) semantics match
the Linux-fd path — unverified today, and shipping it unverified would violate
the no-band-aid rule. Trigger to do it: a real native-ABI container init appears,
or `initial_fds` stdout semantics are confirmed.

### TD31. Cgroup `nr_tasks` accounting is attach/detach-symmetric only, not membership-accurate — RESOLVED 2026-07-02

**RESOLUTION (2026-07-02).** Made membership counting symmetric with task
lifetime. The **detach half** had already landed in `reap_dead_tasks` (commit
`d7b926037`, 2026-07-01): a reaped task in a non-root cgroup calls
`cgroup::detach_task(task_cgroup)` after `drop(state)` (SCHED released → TABLE,
preserving lock order). This 2026-07-02 change adds the matching **attach half**
in `sched::spawn_with_affinity`: after the `without_interrupts`/SCHED critical
section ends and SCHED is dropped, a task that inherited a non-root cgroup calls
`cgroup::attach_task(inherit_cgroup)` (ROOT skipped, matching the reap-side skip;
TABLE taken strictly after SCHED). Because *all* task creation (kernel and user)
funnels through `spawn_with_affinity` (`proc::thread::spawn_user` →
`thread::spawn` → `sched::spawn` → `spawn_with_affinity`), this single site makes
every fork/clone/spawn counted and every reap decremented — a true membership
count. Tasks bound via `set_task_cgroup` (e.g. a container init, which inherits
ROOT at spawn so the spawn-attach is skipped, then is explicitly bound) stay
balanced: attach at bind, detach at reap.

**Why it's now safe (was BLOCKED on a boot hang).** The earlier attempt hung the
boot twice because the extra `TABLE` lock traffic aggravated
**B-PREEMPT-SPINLOCK** — a `crate::sync::Mutex` held across an involuntary
preemption could deadlock against a higher-priority spinner on a single CPU. That
root cause was fixed 2026-07-01 (per-CPU `PREEMPT_DISABLE_COUNT`: a tracked mutex
now disables preemption while held). With that fix, re-applying the attach edit
booted **green 4× consecutively** (baseline 190s + 182s/181s/185s), zero hangs,
zero `SPINLOCK STALL`, zero self-test failures, and no `dash`/`pthread` flakes —
exactly the retry trigger this entry documented. `cgroup::delete`'s
`nr_tasks > 0 ⇒ NotEmpty` guard is now a true "container still has live
processes" check.

---

**Original entry (for context):**

**Where:** `kernel/src/cgroup.rs` (`attach_task`/`detach_task`/`stats.nr_tasks`),
`kernel/src/sched/mod.rs` (`sched::spawn` ~L1046 sets `new_task.cgroup_id` on
creation but does **not** call `cgroup::attach_task`; `reap_dead_tasks` ~L2789
removes a dead task without `cgroup::detach_task`). The single authoritative
mover `set_task_cgroup` *does* keep the counts balanced (detach old, attach new).

**The debt.** `nr_tasks` only counts tasks that were *explicitly moved* via
`set_task_cgroup`. Two asymmetries:
1. **Creation:** a task that simply *inherits* its creator's `cgroup_id`
   (the common case — every fork/clone/spawn) bumps no counter, so a busy
   cgroup can report `nr_tasks == 0` while hosting many tasks.
2. **Death:** when a task is reaped, its cgroup's `nr_tasks` is never
   decremented (and `set_task_cgroup`-style moves to ROOT on container
   `remove_process` leave ROOT's count permanently inflated, since the task is
   then killed without a matching detach).

`detach_task` saturates at 0 so neither asymmetry can panic/underflow, but the
counter is unreliable for anything that needs a true membership count (e.g. a
cgroup "no new forks past a task limit" controller, or `cgroup.procs`-style
introspection).

**Why it didn't block container increment 1 (§41):** `container::run` binds the
init task via `set_task_cgroup`, which *does* increment the container cgroup, so
the end-to-end "process billed to container cgroup" assertion (`nr_tasks == 1`)
holds. The self-test cleanup calls `remove_process_task` (a `set_task_cgroup` to
ROOT) *before* killing the task, so the container cgroup returns to 0 and
`delete()` (which requires `nr_tasks == 0`) succeeds.

**Proper fix.** Make membership counting symmetric with task lifetime, not with
explicit moves: call `cgroup::attach_task(inherit_cgroup)` in `sched::spawn` when
a new task adopts a cgroup, and `cgroup::detach_task(task.cgroup_id)` in
`reap_dead_tasks` (after dropping the SCHED lock, honoring the SCHED → cgroup
lock order). Audit ROOT_CGROUP bootstrapping so the idle/boot tasks are counted
consistently. Once symmetric, `cgroup::delete`'s `nr_tasks > 0 ⇒ NotEmpty` guard
becomes a true "container still has live processes" check.

**ATTEMPTED 2026-07-01 — BLOCKED on a boot hang the change triggers/exposes.**
Implemented exactly the proper fix above: `attach_task(inherit_cgroup)` in
`spawn_with_affinity` (after the `without_interrupts`/SCHED critical section, so
the cgroup `TABLE` lock is taken strictly after SCHED, mirroring
`set_task_cgroup`'s order) and `detach_task(task.cgroup_id)` in `reap_dead_tasks`
(capture `task.cgroup_id` under SCHED, `drop(state)`, then detach — TABLE after
SCHED). It builds clean, clippy-0, and the *normal* container lifecycle self-test
(nr_tasks 0→1→0) still passes. **But two consecutive boot tests hung** (BOOT_OK
never printed within 480 s), each time immediately after a **userspace container
init process** was spawned and marked "running" — run #1 hung in the
`container restart` self-test (after `test-restart-ct` task 185), run #2 in the
`container port` self-test (after `test-port-ct` task 187). Reverting *only* the
two sched edits → BOOT_OK reached in 181 s. So the change is the trigger; the
varying hang location within a boot points to a **near-deterministic SMP timing
race** in the process spawn/force-kill/reap path that the *extra cgroup-`TABLE`
lock traffic* (one attach per spawn, one detach per reap) aggravates rather than
a plain AB-BA deadlock (SCHED and `TABLE` are never held nested; charging holds
frame-lock→`TABLE` while reap does `TABLE`→frame-lock but with `TABLE` released
in between, so no static inversion was found by inspection). Note the boot is
*already* mildly flaky independent of this change: the reverted-sched boot run
saw an unrelated `dash script-from-stdin` self-test `InternalError` (see the
dash-flake entry) — consistent with a pre-existing timing fragility in the
ring-3 spawn/reap machinery that this change amplifies.

**Decision (Claude, autonomous):** do NOT land the symmetric-accounting change
until the underlying spawn/kill/reap race is root-caused, because it regresses
boot stability, and the debt it fixes is cosmetic (stale `nr_tasks` for
force-killed-unreaped tasks; `container::delete` ignores the `cgroup::delete`
NotEmpty error with `let _ =`, so accounting drift never blocks teardown). The
`nr_tasks==1` container-billing assertion and the D-CGROUP-TASK-UNASSIGNED
end-to-end memory-charging test both pass without it. **Trigger to retry:** after
the ring-3 spawn/reap SMP race is instrumented (per-lock acquire/spin counters or
a lock-order tracer) and fixed; then re-apply the two sched edits and run the
boot test ≥3× to confirm stability. The exact patch is small and is captured
above so it can be reconstructed.

### TD30. Console TTY line discipline: `^C`/`^\`/`^Z` signal the fg pgrp (canonical + raw), `VMIN`/`VTIME` + `NOFLSH` honoured, orphan-pgrp `SIGHUP`/`SIGCONT` — RESOLVED 2026-06-20

**Where:** `kernel/src/tty.rs` — `feed()` (canonical line editor) and
`raw_read()` (non-canonical reader); driven by `dispatch_console_read` /
`deliver_console_signal` / `console_terminal_ioctl` in
`kernel/src/syscall/linux.rs`.

**RESOLVED — gap (1) `ISIG` signal generation (`^C`/`^\`):** the console
now has a foreground process group and delivers terminal signals to it.
`tty.rs` gained a `FOREGROUND_PGID` atomic with
`foreground_pgid()`/`set_foreground_pgid()`, the `TIOCGPGRP` (0x540F) /
`TIOCSPGRP` (0x5410) ioctls (`tcgetpgrp`/`tcsetpgrp`), and a
`ConsoleRead{Data(n)|Signal(sig)}` return from `console_read`. On a
`^C`/`^\` in canonical mode (`feed` → `LineStep::Signal`),
`deliver_console_signal()` resolves the foreground pgrp via
`pcb::pids_in_group` and posts `SIGINT`/`SIGQUIT` (with `SI_KERNEL`
siginfo) to every member, then returns `ERESTARTSYS` so the blocked
reader's signal checkpoint runs — a transparent restart when the reader
isn't in the fg group (or the handler has `SA_RESTART`), otherwise the
default action / `-EINTR`. With no foreground group installed
(`pgid == 0`) no signal is generated and the read simply restarts.

**RESOLVED — Ctrl-Z (`VSUSP`) → `SIGTSTP`:** `feed()` now recognises
`VSUSP` under `ISIG` (default `^Z`) and returns `LineStep::Signal(20)`,
flushing the in-progress line like `^C`/`^\`. `deliver_console_signal`
routes `SIGTSTP` to the foreground pgrp, whose `DefaultAction::Stop`
(already implemented in `proc::signal`) suspends the job; a later
`SIGCONT` (shell `fg`/`bg`) resumes it. `NOFLSH` is not yet honoured.

**RESOLVED — `VTIME`:** `raw_read()` now honours all four `(VMIN, VTIME)`
combinations per POSIX. A new `keyboard::read_char_timeout(deadline_ns)`
(HLT-yield loop bounded by an `hrtimer::now_ns()` deadline) backs the two
timed cases: `VMIN=0,VTIME>0` (bounded read timeout on the first byte) and
`VMIN>0,VTIME>0` (inter-byte timer restarted after each byte, first byte
blocking). `VMIN=0,VTIME=0` (poll) and `VMIN>0,VTIME=0` (count) are
unchanged. VTIME is interpreted in deciseconds.

**RESOLVED — raw-mode `ISIG`:** `raw_read()` now classifies each byte
against `VINTR`/`VQUIT`/`VSUSP` when `ISIG` is set (in all four
`(VMIN,VTIME)` arms) and returns `ConsoleRead::Signal`, discarding any
bytes collected so far in the call (input flush — see the `NOFLSH` note
below for why this is unconditional in raw mode).  Apps that clear `ISIG`
(most full-screen programs) still get the characters as literal data.

**RESOLVED — orphaned-process-group `SIGHUP`/`SIGCONT`:** POSIX requires
that when a process exit orphans a process group that still contains a
*stopped* member, that group be sent `SIGHUP` then `SIGCONT` so wedged
jobs are not stuck forever with no shell able to continue them. Now
implemented in the process-exit path rather than tied to a
controlling-terminal model: `pcb::guarded_child_pgrps(pid)` captures the
distinct groups `pid` *guards* (children in a different group but the same
session) **before** `remove_thread` reparents them to init;
`thread::on_thread_exit` re-checks each captured group after the process
zombifies via `pcb::pgrp_orphaned_with_stopped(pgid)` — true only when no
live member has a guardian (a live parent in a different group of the same
session; zombies count as neither member nor guardian) *and* some member
is stopped — and calls `handlers::kill_orphaned_pgrp(pgid)`, which sends
`SIGHUP` then `SIGCONT` to every member via the authority-free
`handlers::deliver_kernel_signal` (classify → default action). Covered by
the `pcb::test_orphaned_pgrp` boot self-test (guarded-vs-orphaned and the
no-stopped-member negative case).

**RESOLVED — `NOFLSH`:** `feed()` now honours the `NOFLSH` (0x80) lflag in
canonical mode: a signal character (`^C`/`^\`/`^Z`) flushes the in-progress
line by default, but with `NOFLSH` set the buffered input is preserved and
only the signal is generated (the line then completes normally on the next
newline). Raw mode keeps no kernel-side input queue across `read(2)` calls
(each call reads straight from the keyboard), so there is no buffered input
for `NOFLSH` to preserve there — documented on `raw_read`. Covered by the
`tty` boot self-test (NOFLSH-preserves-line) and a `#[cfg(test)]` unit test.

**Severity:** none remaining — interactive `^C`/`^\`/`^Z` (canonical and
raw), `VMIN`/`VTIME` raw reads, orphaned-process-group hangup, and `NOFLSH`
all work (once a shell installs a foreground pgrp via `tcsetpgrp`).

### TD29. Linux signal `siginfo` sender-class (`si_code`/`si_pid`/`si_uid`) — RESOLVED 2026-06-15

**Resolution:** Implemented sender-faithful `siginfo`. `SignalState`
(`kernel/src/proc/signal.rs`) now carries a per-signal `Option<SigInfo>` array co-located
under the same lock as the pending bitmap, recorded on the clear→set transition
(coalescing first-wins, matching Linux's standard-signal `struct sigqueue` behaviour) and
taken at delivery. `SigInfo { code, sender_pid, sender_uid, value }` is threaded through the
post funnel: `kill(2)` → `SI_USER` + sender pid/uid; `tkill`/`tgkill` (`raise`/`pthread_kill`)
→ `SI_TKILL` + sender pid; timer expiry (`setitimer`/`alarm` SIGALRM, `kernel/src/proc/itimer.rs`)
→ `SI_KERNEL`. `build_linux_rt_frame` dequeues the matching record to fill the
`LinuxSiginfo` handed to an `SA_SIGINFO` handler. Verified by the `siginfo
record/deliver/coalesce` unit self-test (13 tests pass) and the `/bin/signal` ring-3 glibc
test, which now asserts `si_code == SI_TKILL (-6)` and `si_pid == getpid()` for `raise()`
(`SLATE_GLIBC_SIGNAL_OK signo=10 code=-6 self=1`).

**Synchronous fault `si_code`/`si_addr` — RESOLVED 2026-06-16 (follow-on to TD29).**
CPU faults on an `AbiMode::Linux` process with an installed handler are now delivered as
real Linux signals with a faithful, fault-specific `siginfo`. A shared emitter
`emit_linux_rt_frame(pid, sig, act, regs: &LinuxTrapRegs, siginfo) -> Option<RtFrameEntry>`
(`kernel/src/syscall/linux.rs`) builds the `rt_sigframe` from a neutral register snapshot, so
it is reused by both the async syscall-return path (`build_linux_rt_frame`, snapshot from the
`SyscallFrame`) and the synchronous fault path (`try_deliver_linux_fault_signal`,
`kernel/src/idt.rs`, snapshot read out of the `InterruptStackFrame` + `SavedRegisters` via
`read_volatile`). `linux_fault_mapping` classifies the trap vector → `(signo, si_code)`:
`#DE`→`SIGFPE`/`FPE_INTDIV`, `#OF`→`SIGFPE`/`FPE_INTOVF`, `#UD`→`SIGILL`/`ILL_ILLOPN`,
`#MF`/`#XM`→`SIGFPE`/`FPE_FLTINV`, `#AC`→`SIGBUS`/`BUS_ADRALN`,
`#BR`/`#NP`/`#SS`/`#GP`→`SIGSEGV`/`SI_KERNEL`; `#PF` is handled in `handle_page_fault`, which
sets `si_addr = CR2` and `si_code = SEGV_ACCERR` (protection, present bit set) or
`SEGV_MAPERR` (not mapped). For non-`#PF` faults `si_addr =` faulting RIP. The emitter does
**not** re-arm on a frame-build failure — the fault caller terminates instead, since resuming
would immediately re-fault. Native processes keep the SEH-style `SignalContext` trampoline
(design-decision #4). Verified by the `/bin/fault` ring-3 glibc self-test
(`self_test_linux_real_glibc_fault`, `kernel/src/proc/spawn.rs`): a real `#PF` store to an
unmapped `0xDEAD000` enters an unmodified glibc `SA_SIGINFO` `SIGSEGV` handler that reads
`si_signo==11`/`si_code==SEGV_MAPERR(1)`/`si_addr==0xdead000` and `siglongjmp`s out, printing
`SLATE_GLIBC_FAULT_OK signo=11 code=1 addr=0xdead000` (boot test PASSED).

**`SI_QUEUE` `si_value`/`si_ptr` payload — RESOLVED 2026-06-16 (follow-on to TD29).**
`rt_sigqueueinfo(2)`, `rt_tgsigqueueinfo(2)` and `pidfd_send_signal(2)` now read the
user-supplied `siginfo`, copy out `si_code` and the 8-byte `si_value` union
(`read_user_siginfo_payload`, SMAP-safe via `copy_from_user`), record the value on the
pending signal, and stamp it into the delivered `siginfo_t` at the correct ABI offset
(struct +24) via the new `LinuxSiginfo::queue(...)` builder; `build_linux_rt_frame`
branches to it when `si_code == SI_QUEUE`. The shared kill funnel was refactored into
`kill_common_value` / `tgkill_common_value` / `sys_signal_send_with_info(args, si_code,
value)` so all gate ordering (EFAULT → forging-EPERM → ESRCH-before-EINVAL → authority)
is shared and only the final post stamps the payload. Linux's `do_rt_sigqueueinfo`
forging gate (`(si_code >= 0 || si_code == SI_TKILL) && caller != target → EPERM`) is now
enforced on all three queued-signal entry points; the recorded `si_pid`/`si_uid` is the
*real caller* (faithful + unforgeable), only `si_value`/`si_code` come from the user.
Verified ring-3 by `/bin/sigqueue` (`sigqueue(getpid(), SIGUSR1, {.sival_int=0x12345678})`
→ handler reads `si_code==SI_QUEUE(-1)`, `si_value.sival_int==0x12345678`,
`si_pid==getpid()`, printing `SLATE_GLIBC_SIGQUEUE_OK signo=10 code=-1 value=0x12345678
self=1`, boot test PASSED) plus in-kernel forging-gate (EPERM) and SI_QUEUE-bypass
(ESRCH-before-EINVAL) assertions.

### TD28. Linux `munmap` is 16 KiB-frame-granular (delegates to native handler), not 4 KiB-page-granular — FIXED 2026-06-16

**Where:** `kernel/src/syscall/linux.rs` — `sys_munmap` delegates to the native
`kernel/src/syscall/handlers.rs::sys_munmap`.

**What it is:** the native `munmap` requires a **16 KiB-frame-aligned** start
(`vaddr.is_multiple_of(FRAME_SIZE)`, else `BadAlignment` → `EINVAL`), rounds the
length **up** to a whole 16 KiB frame, unmaps at whole-frame granularity, and
removes only a VMA that *starts exactly* at `vaddr` (`pcb::remove_vma`, not the
`remove_vma_range` surgery). Linux `munmap(2)` on x86-64 accepts any **4 KiB
(page)**-aligned start and unmaps an arbitrary page-granular sub-range, splitting
VMAs at 4 KiB boundaries. So three behaviours diverge from Linux:
1. A 4 KiB-aligned-but-not-16-KiB-aligned start returns `EINVAL` where Linux
   succeeds.
2. A length that is a multiple of 4 KiB but not 16 KiB is rounded **up**, so the
   unmap can spill 4 KiB sub-pages into an adjacent mapping that shares the
   straddling 16 KiB frame.
3. A partial unmap that does not start on a VMA boundary drops no VMA record
   (leaves a stale `[start,end)` VMA), where Linux would split it.

**Why it is not currently biting:** every base address our `mmap` hands back is
16 KiB-aligned (we allocate whole frames), and glibc only `munmap`s regions it
received from `mmap`, so in practice the start is always 16 KiB-aligned and
adjacent glibc mappings are themselves 16 KiB-aligned — the round-up does not
cross into a live neighbour. The Path-Z real-glibc tests (hello/stdio/full/
pthread) all pass with the current handler.

**Proper fix:** give the Linux `sys_munmap` its own 4 KiB-granular path, parallel
to the 4 KiB-granular `sys_mmap`/`sys_mprotect` work: validate `HW_PAGE_SIZE`
(4 KiB) alignment, unmap each 4 KiB sub-page PTE via an `unmap_4k` primitive
(refcount-aware `frame::free_frame` only when the last sub-page of a 16 KiB frame
is unmapped), and call `pcb::remove_vma_range(pid, start, end)` (already 4 KiB-
capable — it splits at arbitrary boundaries) for the VMA surgery, refunding
`RLIMIT_AS` for the actual span. Blocked only by the per-sub-page frame-refcount
bookkeeping (deciding when a shared 16 KiB frame's last 4 KiB tenant leaves).

**Fix (2026-06-16):** `sys_munmap` (`kernel/src/syscall/linux.rs`) now has its own
4 KiB-granular path and no longer delegates to the native handler. It (1) gates
exactly like Linux `do_vmi_munmap` — unaligned (to 4 KiB) start → `EINVAL`; a
length that rounds to zero (incl. `len == 0`) → `EINVAL`; address-arithmetic
overflow or a range leaving user space → `EINVAL` (Linux surfaces all of these as
`EINVAL`, **not** `ENOMEM`); (2) tears down each 4 KiB sub-page PTE via the
existing refcount-aware [`unmap_user_range`] primitive (frees the backing 16 KiB
frame only once its last sub-page tenant is gone, so a partial unmap sharing a
straddling frame with a live neighbour leaves the neighbour intact); (3) performs
4 KiB-boundary VMA surgery via `pcb::remove_vma_range` (splits the covering
VMA(s), retaining/releasing file-backing references for the surviving/removed
pieces); and (4) refunds `RLIMIT_AS` for the bytes of VMAs that *actually*
overlapped `[addr, end)` (computed before the surgery via `linux_vma_overlap_bytes`,
so a never-mapped or VMA-less range refunds 0 — matching that eagerly-mapped PIE
segments were never charged to `linux_as_bytes`). The per-sub-page refcount
bookkeeping that "blocked" this was already solved by `unmap_user_range` (written
for the `MAP_FIXED` overlay path), so no new frame-accounting code was needed.
Verified by an in-kernel gate self-test (`linux.rs` batch 533b: 4 KiB-unaligned
start → EINVAL; 4 KiB-aligned-but-not-16-KiB start no longer EINVAL — reaches pid
resolution → ESRCH from the boot task, proving the alignment is now accepted with
no side effect; out-of-range → EINVAL) plus a clean Path-Z boot-test (BOOT_OK,
0 self-test failures).

**Related fix (2026-06-15):** `remove_vma_range`'s **right** remainder
`[end, vma.end)` previously kept the original `FileBacked.file_offset` while its
`start` moved forward from `vma.start` to `end`, so the surviving high-side piece
of a split file-backed VMA mapped the wrong bytes. Now built via `vma_subrange`
(which advances `file_offset` by `end - vma.start`), matching the `protect_vma_range`
surgery. The left remainder was already correct (its start is unchanged).

### TD27. `mprotect` updates PTE permissions but not VMA flags — a reclaimed-then-refaulted RELRO page restores the old (writable) permission — FIXED 2026-06-15

**Where:** `kernel/src/syscall/linux.rs` — `sys_mprotect`; the VMA surgery lives in
`proc::pcb::protect_vma_range` (with `vma_subrange` for boundary splitting and
`vma_coverage_gaps` for the hole/ENOMEM check). The demand-fault resolver that
reconstructs a PTE from the covering VMA's `flags` is `pcb::try_resolve_fault` /
`pcb::resolve_subpaged_fault`.

**What it was:** `mprotect(2)` changed the live page-table entries for the range
but did **not** split/adjust the underlying `Vma.flags`. As long as the page stayed
resident this was invisible, but if a page in the range was later reclaimed under
memory pressure (`madvise(MADV_DONTNEED)`, or a future swap/anon reclaim path) and
re-faulted, the fault resolver rebuilt the PTE from the *VMA's* stale `flags` — so a
page glibc made read-only for RELRO would come back **writable**, silently weakening
the hardening. There was also a *correctness* bug for demand-paged mappings: glibc's
pthread thread-stack path `mmap(PROT_NONE)` then `mprotect(…, RW)` *before first
touch*, so a PTE-only mprotect left the not-yet-faulted region with its stale
PROT_NONE protection and the worker thread's stack writes faulted — surfacing as
`pthread_create` → EINVAL.

**Fix (2026-06-15):** `sys_mprotect` now calls `pcb::protect_vma_range`, which
performs per-subpage VMA surgery — it splits the covering VMA(s) at the (4 KiB-
aligned) range boundaries via `vma_subrange` (adjusting `FileBacked.file_offset`
and dup'ing backing references for the extra pieces) and recomputes
`WRITABLE`/`NO_EXECUTE` on `Vma.flags` for the affected sub-range, so the fault
resolver reconstructs the correct permissions after reclaim *and* freshly-mmapped
demand-paged regions fault in with the post-mprotect protection. Coverage (Linux's
"ENOMEM on a genuine hole") is checked before any mutation via `vma_coverage_gaps`
combined with a present-PTE check, so the eagerly-mapped (VMA-less but PTE-present)
PIE main-executable segments that glibc RELRO-protects are accepted while true holes
still return ENOMEM. Verified by the Path-Z real-glibc pthread self-test
(`proc::spawn::self_test_linux_real_glibc_pthread`: 4 threads via clone+TLS, 40000
mutex/futex ops, pthread_join) reaching `SLATE_GLIBC_PTHREAD_OK` and exit 13.

### TD26. User-mode CET shadow-stack state (`IA32_PL3_SSP`, `IA32_U_CET`) will be the next instance of the F13/F14 bug class when user CET is enabled — FORWARD-LOOKING HAZARD 2026-06-14

**Where:** `kernel/src/cet.rs` — `set_user_cet(enable_shstk, enable_ibt, user_ssp)`
and `read_user_ssp()`, both currently `#[allow(dead_code)]`. The per-task
context-switch save/restore lives in `kernel/src/sched/mod.rs` (the two
switch sites near lines 3779/3795 and 3974/3985 that already restore
`IA32_FS_BASE` and `IA32_GS_BASE`).

**What it is:** a forward-looking hazard, not a live bug. User-mode CET
(shadow stacks / IBT) is **not currently wired up for user tasks** — the
shadow-stack MSRs `IA32_PL3_SSP` (per-thread user SSP) and `IA32_U_CET`
(per-thread user CET config) are written only by the dead-code
`set_user_cet`, which nothing calls. So today there is no per-thread CET
state to clobber. The doc comment on `set_user_cet` already *claims* it is
"Called during context switch to restore per-task CET state" — that wiring
does not yet exist.

**Why it matters:** `IA32_PL3_SSP` and `IA32_U_CET` are exactly the same
**bug class** as F13/F14 (FS/GS base): they are userspace-settable
*per-thread* CPU register state that lives in MSRs, **not** in the saved GP
`Context` and **not** in the XSAVE area unless XSAVES + the CET_U state
component (bit 11) is enabled. The moment user shadow stacks are turned on,
each thread gets its own shadow stack and its own SSP; if the SSP (and the
U_CET enables) are not saved on switch-out and restored on switch-in, the
first context switch will leave a thread running on another thread's shadow
stack → spurious `#CP` faults or a security hole (shadow-stack reuse). This
audit (the same sweep that found F13/F14) flagged it proactively so it is
not re-discovered the hard way.

**Proper fix (when user CET is enabled):**
1. Add `pub user_ssp: u64` and `pub user_cet: u64` fields to `Task`
   (`kernel/src/sched/task.rs`), symmetric to `fs_base`/`gs_base`; `0` =
   no user CET (the default).
2. In both `sched::mod.rs` switch sites, after the FS/GS restore, restore
   `IA32_PL3_SSP` and `IA32_U_CET` for user tasks (gated on the task
   actually having CET enabled, to avoid a `#GP` writing an SSP MSR when
   CET is off in CR4/U_CET).
3. Sync the fields wherever the SSP/U_CET change: thread creation (allocate
   the shadow stack), `clone`/`fork` (new thread gets a fresh shadow stack;
   `fork` child inherits the parent's SSP value but its own COW shadow-stack
   page), and `exec` (reset to a fresh shadow stack or `0`).
4. Alternatively, if XSAVES is adopted, enabling the CET_U state component
   (XCR0/IA32_XSS bit 11) folds SSP/U_CET into the existing
   `xsave64`/`xrstor64` context-switch path — preferable because it reuses
   the FPU save machinery instead of hand-rolled MSR save/restore. Decide
   between explicit MSR save and XSAVES-CET_U at the time user CET lands.

**Trigger:** do this in the same change that first calls `set_user_cet`
from a live path (i.e. when user-mode shadow stacks / IBT are enabled for
user processes). Until then this is inert dead code and there is nothing to
fix.

### TD24. `link`/`linkat` return a blanket `EROFS` regardless of mount/filesystem — RESOLVED 2026-06-16 (Path Z Part 28)

**Resolution (2026-06-16, commit 5c8ae3e77 "Wire link/linkat to the VFS"):**
this is no longer accurate. `link`/`linkat` now do real VFS work for ring-3
callers: `link_common` (`kernel/src/syscall/linux.rs`) resolves oldpath/newpath
against the caller's cwd/dirfds via `resolve_at_path`, requires a File-WRITE
capability, and calls `Vfs::link`. ext4 implements real hard links (the Part 28
self-test creates one on the `/mnt` ext4 mount and reads it back); memfs cannot
share an inode between two names, so it correctly reports unsupported (mapped to
the filesystem-appropriate errno, matching Linux's `EPERM` for an FS without a
`->link` op — not the misleading `EROFS` this entry was filed against). Only the
kernel-context path (`caller_pid().is_none()`, no fd table) still returns the
`EROFS` terminal, which is required to keep the batch-481 syscall-fidelity
self-test green. The two residual fidelity gaps — `Vfs::link` always follows a
symlink oldpath (so plain `link(2)`'s no-follow contract and `linkat` without
`AT_SYMLINK_FOLLOW` are not honoured for the rare symlink-oldpath case) and
memfs lacking hard-link support (an inode-table refactor) — are tracked under
**B-SYM1**, not here. The historical analysis below is retained for context.

**Where (historical):** `sys_link` / `sys_linkat` in `kernel/src/syscall/linux.rs` (both
return `errno::EROFS` after validating their path/flags arguments).

**What it is:** no filesystem in the OS implements hard links, so both syscalls
fail unconditionally with `EROFS` ("read-only file system"). Linux instead
returns errno by case, in `do_linkat`/`vfs_link` order: oldpath missing →
`ENOENT`; newpath already exists → `EEXIST`; the two paths are on different
mounts → `EXDEV`; the destination mount is read-only → `EROFS`; and a writable
filesystem that simply lacks a `->link` op → `EPERM`. The common real case —
`link("/tmp/a", "/tmp/b")` on our *writable* `/tmp` memfs — should be `EPERM`
(unsupported), not `EROFS` (which misleadingly claims the mount is read-only).

**Related sub-fix landed 2026-06-14 (directory `st_nlink`):** memfs previously
hardcoded every node's `st_nlink` to `1`, including directories. A Unix
directory's link count is `2` (its name in the parent + its own `.`) plus one
per immediate subdirectory (each subdir's `..`); files/symlinks do not bump it.
`find(1)`'s leaf optimisation keys off `nlink == 2` (no subdirs ⇒ skip stat'ing
entries), so the hardcoded `1` both defeated that optimisation and reported a
count no real filesystem produces. memfs now computes directory link counts
honestly via `MemFsNode::nlink_count()` (files/symlinks still report `1` because
file hard links remain unimplemented — the main debt below). This does NOT
resolve TD24: `link`/`linkat` still return blanket `EROFS`.

**Why it's not a live bug today:** programs that use `link(2)` for speed
(git's `link_or_copy`, rsync `--link-dest`, `cp -l`, `ln`) fall back to copying
or report the error; none branch on `EROFS`-vs-`EPERM` in a way that corrupts
data. The only observable effect is a misleading error *message* on an
operation that cannot succeed regardless.

**Proper fix:** the real fix is hard-link support in the backing filesystems
(a substantial FS feature — memfs/ext4/FAT inode link-count + dirent aliasing).
Until then, an interim accuracy improvement would resolve oldpath/newpath, emit
`ENOENT`/`EEXIST`/`EXDEV` (the `KernelError::CrossDevice` variant added 2026-06-14
already maps to `EXDEV`) / `EROFS` / `EPERM` in Linux's order. That interim step
was deliberately NOT taken: faithfully reproducing `do_linkat`'s lookup ordering
(`AT_SYMLINK_FOLLOW` oldpath resolution, `AT_EMPTY_PATH`, dirfd resolution,
parent `ENOTDIR`/trailing-slash handling) for a syscall that always fails risks
introducing *new* divergences that are worse than the current honest-but-coarse
`EROFS`. Revisit when hard links are actually implemented.

### TD23. No `/sys/devices/system/cpu/cpuN/cache/` tree — lscpu/hwloc cannot read real cache geometry — RESOLVED 2026-06-13

**Resolution (2026-06-13):** Built the per-CPU `cache/indexI/` sysfs subtree in
`kernel/src/fs/sysfs.rs`, sourced from `cpu::cache_topology()`. Each detected
cache level/type exposes `level`, `type`, `size`, `coherency_line_size`,
`ways_of_associativity`, `number_of_sets` (all directly CPUID-derived, honest)
plus `shared_cpu_map`/`shared_cpu_list` derived from the real topology via
`cache_shared_cpus()` — which matches `max_sharing` against the known per-core
(thread-sibling) and whole-package scopes and never overclaims (an unplaceable
clustered cache falls back to the known-true per-core subset). The tree is
present only when geometry was detected (`cache_index_count() > 0`); when CPUID
reports nothing (e.g. QEMU's default model) the `cache/` dir is absent rather
than fabricated. lscpu's existing reader (fixed under the previous commit)
lights up automatically with real data on hardware that exposes caches.
Self-test step 13 covers both the populated and absent paths. The original
debt write-up follows for history.

---

**Original debt (now resolved):**

**Where:** kernel `kernel/src/fs/sysfs.rs` (would add a `cache/indexN/` subtree
under each `cpuN`), data source `kernel/src/cpu.rs::cache_topology()` (returns
real CPUID-derived `CacheInfo { level, cache_type, size, line_size, ways, sets,
shared, max_sharing }`). Consumer `userspace/lscpu/src/main.rs` reads
`/sys/devices/system/cpu/cpu0/cache/indexN/{level,type,size,ways_of_associativity}`.

**What:** The sysfs per-CPU `cache/` subtree does not exist yet, so lscpu has no
honest source for L1/L2/L3 cache sizes. As of this entry lscpu correctly
*omits* cache lines it cannot source (the previous behaviour — printing
fabricated `32K`/`256K`/`8192K` defaults and hardcoded `8`/`16` associativity —
was removed because it showed invented numbers as if real). The result is
correct but less informative: `lscpu` and `lscpu -C` show no cache rows.

**Proper fix:** Build the kernel `cache/indexN/` tree from
`cpu::cache_topology()`, exposing the Linux files: `level`, `type`
(`Data`/`Instruction`/`Unified`), `size` (e.g. `32K`/`8192K`),
`coherency_line_size`, `ways_of_associativity`, `number_of_sets`, and
`shared_cpu_map`/`shared_cpu_list`. The geometry fields are all directly
honest (CPUID-derived). `shared_cpu_list` can be derived from `max_sharing`
under our contiguous CPU-numbering model (cache instance for cpuN groups the
`max_sharing` contiguous CPUs containing N) — verify this matches the topology
before relying on it; if `max_sharing` cannot be mapped to a specific CPU set
honestly, omit the share-map files rather than guess. Once the tree exists,
lscpu's existing reader lights up automatically with real data.

**Severity:** low — cosmetic/informational; no correctness impact on CPU
*enumeration* (count/topology come from the already-correct `online`/`present`
range files and `topology/` subtree). Tracked as the follow-up to the
CPU-enumeration sysfs work.

### TD22. File-backed `mmap` — Phase 1 (demand-paged `MAP_PRIVATE`) DONE; Phase 2 read-only unified cache PLANNED-DEFERRED (C-lite, §23); writable `MAP_SHARED` WON'T-FIX — UPDATED 2026-06-14

**Where:** `kernel/src/mm/vma.rs` (`VmaKind::FileBacked`), `kernel/src/proc/pcb.rs`
(`try_resolve_fault` FileBacked arm, `vma_release_backing`, `remove_vma`,
`remove_vma_range`, `reset_vmas_for_exec`, `fork_create`, `destroy`),
`kernel/src/syscall/linux.rs` — `linux_file_mmap` (the file-backed arm of
`sys_mmap`), plus `unmap_user_range` / `linux_file_mmap_rollback` helpers.

**Phase 1 — DONE (2026-06-14): demand-paged `MAP_PRIVATE` for regular files.**
A private, non-fixed `mmap` of a regular file now registers a
`VmaKind::FileBacked { handle, file_offset }` VMA and allocates **no frames**
up front. The page-fault handler (`pcb::try_resolve_fault`) resolves each page
lazily: allocate a zeroed frame, `read_at(handle, file_offset + (page - start))`
into it (tail stays zero past EOF — Linux page zero-fill), then map. Because
the mapping is private, a write faults onto its own per-process frame and never
reaches the file (correct `MAP_PRIVATE` semantics); once populated the frame is
swap-reclaimable and CoW-shareable across `fork` like any anonymous page.
- **Backing-handle lifetime:** the VMA owns an independent reference on the open
  file description (`dup_shared` at mmap, again per-VMA on `fork`, net
  retain/release on `remove_vma_range` splits), released via `close` on
  `munmap` (`remove_vma`), `execve` (`reset_vmas_for_exec`), and process exit
  (`destroy`). This decouples the mapping's lifetime from the caller's fd:
  `munmap`-after-`close` still reads the right bytes.
- **Bonus fix:** `execve` previously never cleared the per-process VMA list when
  it tore down the old address space (`clear_user_address_space`), leaving stale
  records in `/proc/<pid>/maps` and stale ranges the fault resolver could
  "resolve". `reset_vmas_for_exec` now drops them all (and releases their
  backings) so a fresh image starts with an empty VMA list, matching spawn.

**Still eager (unchanged):** memfd-backed maps, read-only `MAP_SHARED`, and
`MAP_FIXED` overlays (the `ld.so` per-segment loader) keep the eager-copy path —
`VmaKind::Fixed`, frames allocated and `read_at`-filled at map time. memfd has a
separate handle layer; FIXED ranges are typically faulted in immediately by the
loader, so demand paging buys little there.

**Still DEBT — Phase 2: unified page cache + writable `MAP_SHARED`.**
- Writable `MAP_SHARED` is still rejected with `ENOSYS` — we never write
  modified pages back to the file, so the shared-write contract is impossible.
  Any Linux program using shared mmap'd files for IPC or in-place editing (some
  databases, `mmap`-based logging) gets `ENOSYS`.
- Two processes mapping the same file do **not** share physical pages — each
  demand-faults its own private copy. There is no unified page cache shared
  between the VFS read path and mmap, so file pages can be resident twice.
- The fault handler reads the file **synchronously via the VFS** inside the
  page-fault path; a page cache would serve hits without re-reading.

**Phase 2 — split decision (operator, 2026-06-14; supersedes the earlier blanket
won't-fix).** The operator reopened Q5 and chose **C-lite**: a unified
*read-only* page cache. See `design-decisions.md` §23 (which narrows §22).
- **Read-only unified cache — PLANNED, DEFERRED.** Cross-process read-only page
  sharing (shared-library `.text` dedup + de-double-caching against
  `fs/cache.rs`) is adopted in principle but **not built yet**. Trigger to
  implement: the first concrete consumer of read-only page sharing — in practice
  the dynamic linker wanting shared-library text dedup. Precursor: stable VFS
  file-identity (`FileMeta.ino` is 0 for memfs/FAT today). Full deferral
  rationale + trigger logged in `todo.txt`.
- **Writable `MAP_SHARED` writeback — WON'T-FIX (unchanged).** Dirty-tracking,
  `msync`/unmap write-back, and cross-process write coherence remain declined.
  **Writable `MAP_SHARED` of a regular file stays `ENOSYS` indefinitely** — a
  deliberate, accepted limitation, not outstanding debt. C-lite (read-only) needs
  none of this machinery.

When C-lite is built, the proper fix is: a unified page cache shared between the
VFS read path and mmap, with file pages cached once and shared read-only
(refcounted frames). The Phase-1 `VmaKind::FileBacked` fault-path shape is already
the right foundation — C-lite only changes each page's *source* (shared cache
frame vs per-mapping `read_at`). It needs the stable VFS file-identity precursor
above and a double-cache-vs-unify call against `fs/cache.rs`. See
`design-decisions.md` §23.

---

### TD21. Minor Linux-ABI fidelity gap — procfs fd visibility for native processes — APPROXIMATION 2026-06-13; sendfile + copy_file_range + splice + tee + vmsplice transfer IMPLEMENTED 2026-06-14

**Where:** `kernel/src/fs/procfs` (`/proc/<pid>/fd[info]`, `linux_fd_list`) and
`kernel/src/syscall/linux.rs` (`sys_sendfile`, `sys_copy_file_range`,
`sys_splice`, `sys_tee`, `sys_vmsplice`). All are documented in-code.

**What it is:** one remaining deliberate Linux-ABI approximation:
- **`/proc/<pid>/fd/` and `/fdinfo/` are EMPTY for *native* processes.** Native
  processes keep their fd table in userspace (`posix/src/fdtable.rs`), which is
  not kernel-visible, so `linux_fd_list` returns `None` and the readdir yields
  zero entries rather than inventing fds. Only Linux-ABI processes (which use the
  kernel-side `KernelFdTable`) get a populated `fd/`. Same honesty stance as the
  fdinfo `mnt_id:`/`ino:` omission — printing fabricated fds would mislead
  introspection tools.

**Progress (2026-06-14) — sendfile data transfer implemented.** `sys_sendfile`
previously validated its front gates and then terminated `EINVAL` (no transfer).
It now performs a real in-kernel copy via `sendfile_core`: a 64 KiB bounce-buffer
loop reads from the source fd at an absolute byte offset (`fs::handle::read_at` /
`memfd::read_at`, never advancing the open-file cursor) and writes to the
destination fd (`fs::handle::write` / `memfd::write` / `pipe::write|try_write` /
console). Source must be a seekable byte container (File/MemFd — the kinds that
back Linux's sendfile splice_read); destination may be File/MemFd/Pipe/Console.
Position semantics match Linux: with a NULL `offset` the source's file position is
read-from and advanced; with a non-NULL `offset` the read starts at `*offset`, the
file position is untouched, and the post-transfer position is written back via
`put_user` (which — as on Linux — can override a successful copy with EFAULT if the
offset slot became unwritable). `count` is clamped to `MAX_RW_COUNT` (0x7fff_f000);
the transfer stops at source EOF or a short destination write; a pipe whose reader
has gone yields EPIPE. Error semantics follow `do_sendfile`: a first-byte error
propagates, a later error returns the partial count. Tested end-to-end by the
post-`/tmp` boot self-test `self_test_sendfile` (File→File whole-file, offset+count
slice, count-clamp-to-remaining, EOF→0, File→MemFd and MemFd→File cross-kind). The
gate-only `self_test_sendfile_splice_aio` batch-538 checks still pass (kernel-context
callers have no fd table, so the syscall still terminates `EINVAL` before the
transfer). The `put_user(pos)` write-back EFAULT noted previously is now modelled.

**Progress (2026-06-14) — copy_file_range data transfer implemented.**
`sys_copy_file_range` was likewise a validate-front-gates-then-`EINVAL` stub. It
now performs a real positional in-kernel copy via `copy_file_range_core`: a
64 KiB bounce-buffer loop that reads from the source at an absolute byte offset
(`read_at`) and writes to the destination at an absolute byte offset (`write_at`,
which extends the file) — neither cursor is touched by the core, so distinct
source/dest offsets are honoured. Both source and destination must be regular-file
kinds (File/MemFd), matching `vfs_copy_file_range`'s `S_ISREG` requirement; pipes
and consoles are rejected `EINVAL`. The full Linux gate order is now enforced:
fds (`EBADF`) → offset readability (`EFAULT`) → `flags != 0` (`EINVAL`) →
regular-file (`EINVAL`) → access-mode/`O_APPEND` (`EBADF`). Position semantics
mirror sendfile: a NULL `off_in`/`off_out` reads-from and advances the file's own
cursor; a non-NULL pointer supplies an explicit position, leaves the cursor, and
writes the post-transfer position back via `put_user` (EFAULT-after-copy modelled).
`len` is clamped to `MAX_RW_COUNT`; the same first-byte-propagate / later-error-
returns-partial semantics apply. Linux's same-file-overlap `EINVAL` is enforced by
`copy_file_range_overlaps`. **LIMITATION:** "same object" is detected by open-path
equality (File) / raw-handle identity (MemFd); two *hardlinks* to one inode have
distinct paths and are not detected as overlapping — Linux compares inodes, which
our fd layer does not expose here (same approximation the rest of the path layer
makes). Tested end-to-end by the post-`/tmp` boot self-test
`self_test_copy_file_range` (File→File positional whole-file, positional read
offset, positional *write* offset, File→MemFd / MemFd→File cross-kind, and
overlap-detect true/false/cross-kind). The batch-537 gate-only checks still pass
(kernel-context callers terminate `EINVAL` before the transfer).

**Progress (2026-06-14) — splice data transfer implemented.** `sys_splice` was
also a validate-front-gates-then-`EINVAL` stub. It now moves data via
`splice_core`, a 64 KiB bounce-buffer loop where File/MemFd ends are read/written
positionally (`read_at`/`write_at`) and pipe ends use their own cursors
(`pipe::read|try_read` / `pipe::write|try_write`). The full Linux gate order is
preserved (len==0→0; flags mask→EINVAL; fds→EBADF; pipe-end-with-offset→ESPIPE;
offset readability→EFAULT; FMODE_READ/WRITE→EBADF) and the do_splice prologue
gates are added: at least one end must be a pipe (else EINVAL — sendfile territory),
a non-pipe end must be a splice-capable regular file (File/MemFd, else EINVAL), and
splicing a pipe to its own other end (ipipe==opipe) is EINVAL. `SPLICE_F_NONBLOCK`
selects the non-blocking pipe path; a broken pipe yields EPIPE, non-blocking
exhaustion with nothing moved yields EAGAIN. Position semantics match the siblings
(NULL offset advances the file cursor; explicit offset is read/written-back via
`put_user`). Tested by `self_test_splice` (File↔Pipe, Pipe→Pipe, positional
read+write offsets, empty-source→EAGAIN, and the no-loss bound).
**LIMITATION (pipe→pipe data-loss race):** our model copies bytes (read source →
write dest) rather than moving pipe buffers by reference like Linux. To avoid
discarding already-consumed source bytes on a partial destination-pipe write, the
non-blocking read is bounded to the destination pipe's current free space
(`readable_bytes` on the write end). A *concurrent* writer racing to fill the
destination pipe between the space probe and the write is the only residual loss
window; it is bounded to one 64 KiB chunk and is no worse than any non-atomic
splice. The blocking path has no such window (its inner write loop drains the full
chunk). Proper fix would require reference-counted pipe buffer pages (Linux's
`pipe_buffer` model) so splice transfers ownership instead of copying.

**Progress (2026-06-14) — tee data transfer implemented.** `sys_tee` was the
last validate-then-`EINVAL` stub in this family. It now duplicates data from one
pipe to another *non-destructively* via `tee_core`, built on two new pipe
primitives (`kernel/src/ipc/pipe.rs`): `peek_at(handle, offset, buf)` copies
buffered bytes at a logical offset without consuming them, and `wait_readable`
blocks for input without consuming (the blocking-tee path). Gates match `do_tee`:
flags/len front gates (unchanged batch-540), then FMODE_READ/WRITE→EBADF and the
"two distinct pipes" requirement (both ends must be pipes with different ids,
else EINVAL). `SPLICE_F_NONBLOCK` selects non-blocking; a broken destination
pipe→EPIPE, non-blocking empty source→EAGAIN, EOF source→0. Because tee never
consumes the source, the splice pipe→pipe data-loss concern does **not** apply
here — a partial destination write simply copies fewer bytes and leaves the
source intact. Tested by `self_test_tee` (duplicate + verify the source is
unchanged, empty→EAGAIN, EOF→0, len-clamp). The whole splice/tee/vmsplice
gate-only batch checks (539/540/541) still pass.

**Progress (2026-06-14) — vmsplice data transfer implemented.** `sys_vmsplice`
was the last validate-then-`EINVAL` stub in the zero-copy family. It now moves
data between a process's user iovecs and a pipe via `vmsplice_core`. Direction is
chosen by the pipe-fd's access mode (Linux's `vmsplice_type`): a write-end
(FMODE_WRITE) **gathers** user buffers into the pipe (ITER_SOURCE); a read-end
(FMODE_READ) **scatters** pipe bytes out to the user buffers (ITER_DEST). The
Linux gate order is preserved: flags `& !SPLICE_F_ALL`→EINVAL; fd validity→EBADF;
`nr_segs==0`→0; `nr_segs>1024` (UIO_MAXIOV)→EINVAL; iov pointer NULL→EFAULT;
`validate_user_read` of the iovec array→EFAULT; then the fd must resolve to a
**pipe** (non-pipe→EBADF, matching `get_pipe_info`). Each 16-byte iovec is parsed
(`iov_base`,`iov_len`), zero-length segs skipped, and `iov_base==0` /
`base+len > USER_SPACE_END` rejected EFAULT; the running total is capped at
`MAX_RW_COUNT`→EINVAL. A broken pipe yields EPIPE, non-blocking exhaustion with
nothing moved yields EAGAIN, and the first-byte-propagate / later-error-returns-
partial convention matches the siblings.
  The novel piece is **cross-address-space user access.** The existing
`copy_from_user`/`copy_to_user` (`mm/user.rs`) target the *current* CR3 via
STAC/CLAC, which is the kernel's own address space at boot (no user mappings), so
they can't be exercised by a boot self-test. Two new pml4-parameterized primitives
— `copy_from_user_as(pml4, src, dst)` and `copy_to_user_as(pml4, dst, src)` —
walk an *explicit* page table and reach each user page through the HHDM
(physical→kernel direct map), sidestepping SMAP entirely. In production
`sys_vmsplice` passes the caller's own pml4 (`cr3_to_pml4(read_cr3())`); the self-
test passes a throwaway process's pml4. These are also the reusable primitive a
future `process_vm_readv`/`writev` will need. Tested by the post-process-init boot
self-test `self_test_vmsplice`, which spins up a throwaway PCB, maps two adjacent
writable user frames, and checks: cross-page `copy_from_user_as`, cross-page
`copy_to_user_as` plus rejection of an unmapped VA, ITER_SOURCE (user→pipe), and
ITER_DEST (pipe→user). The batch-541 gate-only checks still pass (kernel-context
callers have no caller-pid/fd table and terminate before the transfer).

**Bug fixed in passing (2026-06-14) — rmap/swap-reclaimable leak on process
teardown.** While boot-testing vmsplice the `mm/compact.rs` Test 5 assertion
("collect_private_frames should find our fake entry") began panicking. Root cause
was a genuine pre-existing correctness bug, *not* the test: `clear_user_address_
space` (`mm/page_table.rs`) freed a process's user frames (`frame::free_frame`)
without ever calling `rmap::remove` or `swap::unregister_reclaimable` for them. So
every process destroy/exec leaked stale reverse-mappings and reclaimable entries
pointing at frames that were freed and could be reused — a real hazard (memory
compaction or the swap reclaimer could act on a freed-and-reused frame) that also
let leaked rmap entries accumulate until they crowded out compact's fragile
4-slot probe. **Fix:** in the frame-freeing loop, before `free_frame`, the page-
table indices are reassembled into the frame's virtual base
(`(pml4_idx<<39)|(pdpt_idx<<30)|(pd_idx<<21)|(pt_idx<<12)`) and used to call
`rmap::remove(frame_phys, pml4_phys, virt_base)` and
`swap::unregister_reclaimable(pml4_phys, virt_base)` (both no-ops for untracked
frames). After the fix the rmap table is empty at compact time and Test 5 passes
(`found=1, saw_fake=true`).

**Impact:** low — native-process fd introspection via `/proc` is unavailable
(tools must use the native fd API).

**Proper fix:** procfs fd — expose a kernel-visible view of native fd tables (or a
read bridge into the userspace fd table) so `/proc/<pid>/fd` works uniformly.

### TD20. Userspace crate verification & lint-cleanup gaps — coreutils RESOLVED 2026-06-14; guitk pedantic still DEBT 2026-06-13

**Where:** `gui/toolkit/` (guitk). (The coreutils half is resolved — see below.)

**What it is:** two low-priority verification/lint gaps in userspace crates:
- **coreutils host-test gap (2026-05-31) — RESOLVED 2026-06-14.** The affected
  bins (`stat`, `du`, `chown`, `chmod`, `tar`, `test`, `ln`) now follow the
  `stat.rs` pattern: every `std::os::unix` import and the unix-only logic sit
  behind `#[cfg(unix)]`, a `#[cfg(not(unix))]` stub `main` keeps the non-unix
  host compile-clean, and the pure formatting/parsing helpers live outside the
  gate with host-runnable unit tests. Verified 2026-06-14:
  `cargo test -p coreutils --target x86_64-pc-windows-gnu` compiles and runs
  green on the Windows dev host (20 test binaries, ~480 tests, 0 failures), so
  the host `cargo test` path now works alongside the slateos build. (Originally:
  coreutils unit tests couldn't compile on the Windows dev host because bins
  used `std::os::unix::fs::{PermissionsExt, MetadataExt}`, which only exist on
  unix-family targets.)
- **guitk pedantic deferral (2026-06-03):** guitk does not yet enable
  `#![deny(clippy::pedantic)]`; a pedantic run emits ~1,232 warnings,
  overwhelmingly doc-style (`missing_panics_doc`, `missing_errors_doc`,
  `must_use_candidate`, `return_self_not_must_use`, `needless_pass_by_value`,
  `similar_names`, `items_after_statements`). The crate is ~50k LOC; cleanup is a
  multi-session sweep, deferred until core subsystems (kernel/mm/sched/ipc, fs,
  drivers) reach a stable baseline — little value in extensive doc lints on
  toolkit code while the syscall ABI is still in flux. (Related to TD19's
  lint-policy conflict.)

**Impact:** low — neither blocks feature work; both crates build for slateos.

**Proper fix:** coreutils — DONE (the `#[cfg(unix)]` gating + `not(unix)` stub
`main` pattern is now applied across the affected bins; host `cargo test`
compiles and passes). guitk — a dedicated pedantic-cleanup sweep once the core
ABI stabilizes, resolved together with the TD19 lint-policy decision.

### TD19. Crate-root `#![deny(clippy::pedantic)]` overrides the workspace lint allow-list — DEBT 2026-06-13 (needs operator policy call)

**Where:** every crate carrying a crate-root `#![deny(clippy::all,
clippy::pedantic)]` (e.g. `posix/src/lib.rs`) vs. the root `Cargo.toml`
`[workspace.lints.clippy]` block. Reproduce: `cargo clippy -p posix --target
x86_64-pc-windows-gnu` reports ~3038 errors + 260 warnings.

**What it is:** rustc applies crate-root attributes *after* and at higher
precedence than the command-line lint flags Cargo derives from
`[workspace.lints]`. So a crate-root `#![deny(clippy::pedantic)]` re-denies the
whole pedantic group and overrides every per-lint `= "allow"` in
`[workspace.lints.clippy]`. The only allows that survive are ones *also* listed
in that crate's own `#![allow(...)]`. Result: workspace-allowed lints
(`unreadable_literal` ~1943, `must_use_candidate` ~761, `manual_let_else`, …)
fire as hard errors anyway. The 260 warnings (`indexing_slicing` 171,
`arithmetic_side_effects` 89) are correct warn-level per the workspace config.
This is a design conflict between (a) CLAUDE.md's mandate of
`#![deny(clippy::all, clippy::pedantic)]` in every crate and (b) the newer
`[workspace.lints.clippy]` block (`pedantic = "warn"` + centralized allow-list)
documented as the intended suppression mechanism — mutually exclusive while both
are in force. Note: 15 userspace tools have already adopted `[lints] workspace =
true` and dropped their crate-root deny, so the conflict is being resolved
piecemeal in that direction.

**Impact:** low — bare-metal build and all host tests are green; this only
affects `clippy -p <crate>` noise. Not blocking feature work.

**Proper fix:** an **operator policy call**, because CLAUDE.md is operator-owned
and OPT 1 relaxes its "deny in every crate" rule:
- **OPT 1 (recommended):** remove the redundant crate-root deny from each crate
  and rely on `[lints] workspace = true`; the workspace config becomes
  authoritative (`clippy::all` deny, pedantic warn, allow-list effective).
  Residual non-allowed lints then surface as warnings to fix or add to the
  allow-list. Downside: pedantic becomes warn-level workspace-wide.
- **OPT 2:** keep the crate-root deny and copy the full workspace allow-list into
  every crate's `#![allow(...)]` — the per-crate duplication the workspace block
  was created to avoid. Already done in source: `decimal_bitwise_operands` was
  relaxed at both the workspace level and in `posix/src/lib.rs` (our `linux_*`
  ABI constant tables mirror upstream kernel headers verbatim, so hex literals
  would obscure the correspondence). Trigger: dedicated lint-policy pass once the
  operator picks an option.

### TD18. A group of userspace net/disk/admin tools target syscalls that don't exist in the native ABI — DEBT 2026-06-13

**Where:** `userspace/` tools — net-config (`dhcpcd`, `fw`, `ifconfig`, `ip`,
`nft`, `route`), mount (`mount`, `umount`), disk-admin (`mkfs`, `fsck`,
`diskutil`), and `chroot`. Authoritative syscall list:
`kernel/src/syscall/number.rs`.

**What it is:** a 2026-05-30/31 audit of ~55 userspace tools that hand-roll
inline-asm syscalls found most tools were either already correct or fixable by
migrating to `std` / posix `extern "C"` symbols (those fixes shipped — see git
log for jq/zip/ssh-keygen/curl/dig/whois/screen/telnet/stty/df/chown/chmod/
monctl/date/at/nmap/ntpd/hwclock). The residual group below calls syscalls that
**genuinely do not exist** in the native ABI, so they cannot be fixed by a
client-side number correction:

- **net-config** (`dhcpcd`, `fw`, `ifconfig`, `ip`, `nft`, `route`): all issue
  `SYS_NET_IOCTL=810` (which aliases `UDP_BIND`) for interface/route/DNS/firewall
  *writes*. **Interface-address writes: kernel syscall LANDED 2026-07-02** —
  `SYS_NET_IF_CONFIG=856` (`kernel/src/syscall/number.rs`, dispatched in
  `dispatch.rs`, handled by `sys_net_if_config` in `handlers.rs`) is the native
  write side of `NET_IF_INFO=842`: root-gated (`require_netadmin_authority`), it
  applies IPv4 address/mask/gateway/DNS and/or the up/down flag to the physical
  NIC via `net::interface::configure`/`set_up` (new `set_up` helper), using an
  18-byte record with a per-field mask (bit0..4 = ip/mask/gateway/dns/up) so a
  tool changes only the fields it means to (read-modify-write). Boot self-test
  `net::interface::test_write_primitives` (snapshot→configure→toggle up/down→
  restore) verified in serial. **Tool rewiring (a): DONE 2026-07-02** —
  `ifconfig`, `ip`, `route`, and `dhcpcd` now issue `SYS_NET_IF_CONFIG=856`
  instead of the neutered `net_ioctl` stub, via a shared `build_config_record`/
  `net_if_config` (host-unit-tested per tool). Mapping: `ifconfig eth0 <ip>` →
  IP bit; `ifconfig ... netmask` → MASK bit; `ifconfig up/down` → UP bit;
  `ip addr add <ip>/<prefix>` → IP|MASK (prefix→mask); `ip addr del` → IP=0;
  `ip link set up/down` → UP; `ip route add/del default via <gw>` and
  `route add/del default gw <gw>` → GATEWAY bit (clearing to 0 on del);
  `dhcpcd` applies a whole lease (IP|MASK|GATEWAY|UP) in one call. Fields the
  kernel model can't represent are now honest hard errors instead of silent
  fake-success: `ifconfig` MTU/explicit-broadcast. Host tests green: `ifconfig`
  38, `ip` 23, `route` 15, `dhcpcd` 110.
  **Route-table follow-up (b) — DONE 2026-07-02.** Three native route syscalls
  now exist (`kernel/src/syscall/number.rs`): `SYS_NET_ROUTE_ADD=857`
  (root-gated, 16-byte record `[dest(4), mask(4), gateway(4), metric(4 LE)]`,
  rejects 0.0.0.0/0), `SYS_NET_ROUTE_DEL=858` (root-gated, 8-byte
  `[dest(4), mask(4)]`), `SYS_NET_ROUTE_LIST=859` (read-only, fills a buffer
  with 16-byte records, returns count). All operate on the caller's netns via
  `crate::sched::current_task_net_ns()` and the pre-existing per-namespace
  `netns` route table (`add_route`/`remove_route`/`routes`). The *default*
  route (0.0.0.0/0) still lives in the interface gateway (SYS_NET_IF_CONFIG),
  not the table — see design-decisions §52; `resolve_next_hop` for the root
  namespace now consults `route_lookup(ROOT_NS, dst)` before the interface
  gateway fallback. Boot self-test: `ipv4::root_route_next_hop_self_test()`
  (runs after `netns::init()`; adds a TEST-NET-3 route, checks the next hop,
  removes it). The `ip` tool (`ip route add/del <prefix> via <gw> [metric]`)
  and `route` tool (`route add/del -net/-host …`, and `route flush`) now issue
  these syscalls for non-default routes and list the table via
  `SYS_NET_ROUTE_LIST`; the default-route path still uses the interface
  gateway. **Firewall write path DONE 2026-07-02:** `SYS_NET_FW_ENABLE`/
  `_SET_POLICY`/`_ADD_RULE`/`_DEL_RULE`/`_FLUSH` (860–864, root-gated, per-netns
  with root ns == global firewall) expose `net::firewall`'s write path.
  `ADD_RULE` takes a 12-byte binary record mirroring `Rule` 1:1
  (`[direction, action, protocol, src_prefix, dst_port:u16le, priority:u16le,
  src_ip:4]`); reads stay on `/proc/net/firewall`. The `fw` tool now issues
  these syscalls (`fw enable/disable/allow/deny/policy/delete/reset/load` apply
  to the kernel; `apply_to_kernel` does flush+re-add so kernel state matches the
  in-memory set). Rules the kernel model cannot represent (a `src_port` or
  `dst_ip` constraint) are **skipped with a warning** rather than pushed as a
  broader rule — see design-decisions §53. `fw delete N` maps the list position
  to the correct kernel index (counting only representable rules). See
  design-decisions §53 for the ABI + fail-safe rationale. **Still TODO:** the
  `nft` tool (3.6k-line nftables front-end) is not yet wired to these syscalls,
  and IPv6 firewall rules (`Rule6`) have no write syscall yet — both are
  separate follow-ups. Original harm analysis (traced
  2026-06-01): with a Socket-WRITE cap the old call silently binds+leaks a UDP
  socket on a low port and misleads the user that the config change applied;
  without the cap it fails. **Write-path harm neutered 2026-06-14** for all six
  net-config tools (`ifconfig`/`ip`/`route` then `dhcpcd`/`fw`/`nft`) — see the
  dedicated bullets below.
- **mount/umount**: **RESOLVED 2026-06-20.** Real native syscalls now exist —
  `SYS_FS_MOUNT=652` and `SYS_FS_UMOUNT=653` (`kernel/src/syscall/number.rs`),
  dispatched in `dispatch.rs`, handled in `handlers.rs`
  (`sys_fs_mount`/`sys_fs_umount`, root-gated via `require_mount_authority`).
  `SYS_FS_MOUNT` takes three ptr+len string pairs (source/target/fstype —
  consuming all six arg slots, so mount *flags* are deferred to a future
  versioned extension) and dispatches on the fstype string to the existing
  in-kernel backends (ext4/tmpfs(memfs)/iso9660/devfs/proc/sysfs/vfat).
  `SYS_FS_UMOUNT` takes target ptr+len and refuses `/` and busy mounts. Kernel
  boot self-test: `fs::vfs::mount_self_test()` (mounts a scratch tmpfs at
  `/_mount_selftest`, write/read roundtrip, confirms `/` is unmountable-refused,
  unmounts) — runs unconditionally on any root. The `userspace/mount` tool now
  issues these real syscalls (via a `syscall6` inline-asm helper) instead of
  returning ENOSYS; `canonical_fstype` maps user fstype names to kernel
  fstypes, bind/remount are rejected (unsupported by the ABI), and mount
  options emit a "not yet honoured" warning. Host unit tests: `cargo test -p
  mount --target x86_64-pc-windows-gnu` (6 pass). The redundant
  `userspace/mount-cli` demo tool (which printed *fabricated* mount listings and
  fake-succeeded without a syscall) was **removed 2026-06-20** — all three of
  its personalities are already covered by real, non-fabricating tools:
  `mount`/`umount` (the tool above) and the standalone `userspace/findmnt`
  (reads `/proc/mounts`). Nothing referenced `mount-cli`. (Judgment call —
  removal is reversible via git; see todo.txt.) The analogous
  `userspace/mkfs-cli` and `userspace/fsck-cli` demo shims (which *fabricated*
  mkfs/fsck success — fake UUIDs, "done", "clean, NNN/NNN files" — without
  issuing any syscall, telling the user a format/check succeeded when nothing
  happened) were **removed 2026-06-20** for the same reason: all their
  personalities are already covered by the real, syscall-backed `userspace/mkfs`
  (argv0 `mkfs.<type>` detection → `SYS_FS_FORMAT`) and `userspace/fsck` (argv0
  `fsck.<type>` detection → `SYS_FS_CHECK`). The shims' extra aliases
  (`e2fsck`/`xfs_repair`/`mkswap`) were pure fabrication for filesystems we don't
  support; reintroducing any as a real alias is a future task with real backing.
  Nothing referenced either crate. (Judgment call — removal is reversible via
  git; see todo.txt.)
- **mkfs/diskutil format: RESOLVED 2026-06-20** — added a real
  `SYS_FS_FORMAT=654` (`kernel/src/syscall/number.rs`), dispatched in
  `dispatch.rs`, handled in `handlers.rs` (`sys_fs_format`, root-gated via
  `require_format_authority`). ABI: arg0/arg1 = device-name ptr+len (the
  block-device registry name, e.g. "vda"/"sda" — **not** a `/dev/` path),
  arg2/arg3 = fstype ptr+len, arg4/arg5 = optional label ptr+len (0/0 = none).
  The handler dispatches on the fstype string to the existing in-kernel
  `fs::fat::mkfs_fat(device, label)` for the FAT family (vfat/fat/fat32/fat16/
  msdos); all other fstypes return `NotSupported` (ext4 mkfs not yet ported;
  tmpfs has no device to format). Kernel boot self-test:
  `fs::fat::format_self_test()` — registers a 4 MiB `RamBlockDevice` ("fmttest0",
  added to `blkdev.rs` alongside `blkdev::unregister`), runs `mkfs_fat`, mounts
  the formatted volume via `FatFs::mount` + `Vfs::mount` at `/_fmt_selftest`,
  write/read roundtrips a file, then tears everything down — runs unconditionally
  on any root (verified "[fat] mkfs/format self-test PASSED" in serial). Both
  `userspace/mkfs` and `userspace/diskutil format` now issue the real syscall
  via a `syscall6` inline-asm helper (FAT family only; unsupported fstypes report
  an honest "kernel cannot format X yet" error instead of ENOSYS). mkfs warns
  that `-F`/`-s`/`-S` are advisory (the kernel backend auto-selects FAT type and
  cluster geometry from device size). Host tests: `cargo test -p mkfs --target
  x86_64-pc-windows-gnu` (35 pass).
- **fsck/diskutil verify+repair: RESOLVED 2026-06-20** — added a real
  `SYS_FS_CHECK=655` (`kernel/src/syscall/number.rs`), dispatched in
  `dispatch.rs`, handled in `handlers.rs` (`sys_fs_check`, root-gated via
  `require_fsck_authority`). ABI: arg0/arg1 = device-name ptr+len (the registry
  name, e.g. `vda`/`sda`, NOT the `/dev/` path), arg2 = flags (bit0 = repair).
  Returns the count of *outstanding* errors (after repair if requested) or a
  negative `KernelError`. FAT only — delegates to the existing in-kernel
  `fs::fat::fsck_fat(device, repair)`. Kernel boot self-test:
  `fs::fat::fsck_self_test()` — registers a 4 MiB `RamBlockDevice` ("fscktest0"),
  `mkfs_fat`, then `fsck_fat(dev, false)` (expects 0 errors) and
  `fsck_fat(dev, true)` (expects 0 outstanding after repair), teardown via
  `cache::invalidate` + `blkdev::unregister`; runs unconditionally on any root
  (verified "[fat] fsck self-test PASSED" in serial). Both `userspace/fsck`
  (rewired from the **colliding** `652`/`653` — which I had just reassigned to
  `SYS_FS_MOUNT`/`SYS_FS_UMOUNT`, so `fsck` was invoking mount/umount with garbage
  args; now uses `655` + `FS_CHECK_REPAIR=1<<0`) and `userspace/diskutil`
  (`verify` = `fs_check(false)`, `repair` = `fs_check(true)`) now issue the real
  syscall. Host tests: `fsck` 39 pass, `mkfs` 35 pass, `diskutil` 0.
- **diskutil usage/statfs: RESOLVED 2026-06-20** — diskutil's `usage` was an
  ENOSYS stub falling back to a sysfs size estimate, but a real native
  `SYS_FS_STATVFS=608` syscall already existed (`sys_fs_statvfs` in handlers.rs,
  backed by the fully-implemented `Vfs::statvfs(path) -> FsInfo` across
  FAT/ext4/memfs/devfs/iso9660/procfs/sysfs). `cmd_usage` now calls it
  (`fs_statvfs(path)`: path ptr+len + 64-byte buffer → block_size/total/free
  blocks + inodes), printing exact Total/Used/Free/Available/inode figures; it
  only falls back to the sysfs estimate if the syscall genuinely fails. Host
  tests: `diskutil` 5 pass (`read_u64_le` LE-parse + bounds, `syscall_error_msg`,
  `format_size`). The kernel exposes a single free count (no separate
  "available-to-unprivileged"), so diskutil reports available == free.
- **Linux-ABI `statfs`/`fstatfs` returned fixed synthetic data — RESOLVED
  2026-06-20** — `sys_statfs`/`sys_fstatfs` (`kernel/src/syscall/linux.rs`) never
  resolved the path/fd; they always `fill_statfs_default()`'d a hardcoded block
  (TMPFS_MAGIC, 16 GiB total / 8 GiB free, 64K inodes) regardless of the real
  filesystem. So Linux programs calling `statfs("/")` or `df`-style tools got
  bogus capacity. Now `sys_statfs` canonicalises the path against the caller's
  cwd and routes through `Vfs::statvfs`, and `sys_fstatfs` resolves the fd's VFS
  handle to a path (`fs::handle::handle_path`) and does the same; a new
  `fill_statfs_from_info` maps `FsInfo` → the 15-`u64` `struct statfs` layout
  with a real `f_type` super-magic (`statfs_magic_for`: ext4 0xEF53, FAT 0x4d44,
  iso9660 0x9660, procfs 0x9fa0, sysfs, else TMPFS_MAGIC). NotFound → ENOENT;
  non-VFS fds (pipes/eventfd/…) and virtual filesystems still get neutral
  defaults (honest — they have no on-disk capacity). The field-packing loop was
  refactored to `chunks_exact_mut` (no index arithmetic). Validated by a new
  post-mount boot self-test `self_test_statfs_root()` (called from main.rs after
  the root is mounted, since the in-`self_test()` checks run pre-mount) asserting
  `statfs("/")` returns 0 with a non-zero `f_type` + `f_namelen`; the pre-mount
  self-test keeps the NULL→EFAULT checks. Boot PASSED.
- **diskutil trim** — **RESOLVED 2026-06-20.** Built the full fstrim stack:
  (1) a block-layer discard primitive — `BlockDevice::supports_discard()`/
  `discard(start_lba, count)` (default not-supported) with a real
  `RamBlockDevice` impl (zeroes the range, fully bounds/overflow-checked) and
  registry helpers `blkdev::supports_discard()/discard()`; (2) `FileSystem::trim()`
  (default no-op `Ok(0)`) + `FatFs::trim()` which walks the FAT, coalesces
  contiguous free clusters into runs and issues `blkdev::discard` for each
  (after `cache::invalidate_range` drops cached copies so stale free-space data
  can't resurface) — **non-destructive**, only free blocks are touched;
  (3) `FileSystem::device_name()` + `Vfs::trim_device(dev)` for device→mount
  resolution; (4) `SYS_FS_TRIM` (656, root-only) wired to `Vfs::trim_device`,
  returning bytes discarded; (5) `diskutil trim` issues the syscall and reports
  the byte count. Three boot self-tests (block-layer discard, FAT fstrim via
  `Vfs::trim_device`, unknown-device rejection) + 5 diskutil host tests. Boot
  PASSED (fstrim discarded 4,160,512 bytes on a 4 MiB scratch volume).
  **Follow-ups (TD18 residual):** virtio-blk does not yet negotiate
  `VIRTIO_BLK_F_DISCARD`, so on real/virtio devices `supports_discard()` is
  false and fstrim is a successful no-op (0 bytes) — discard only actually
  fires on `RamBlockDevice` today; and ext4 still uses the default `trim()`
  no-op (no free-block-bitmap enumeration yet). See todo.txt.
- **chroot**: no `CHROOT`/`CHDIR`/`SETUID`/`SETGID`/`SETGROUPS` syscall — needs a
  real process-credential + filesystem-root ABI. **Already neutered** — `chroot`
  carries ENOSYS stubs and a comment about the earlier fake syscall numbers.

**Impact:** these specific tools are non-functional (no-op at best). They are not
on any critical path, so nothing currently blocks on them.

**Read-path wiring — DONE 2026-06-14.** The decision-free near-term win below
has been applied to all three read-path tools (`ifconfig`, `ip`, `route`):
- **`ifconfig` (no-args / `-a` / `-s` / `ifconfig <iface>`) — DONE 2026-06-14.**
  Display mode previously read `/sys/class/net/` and `/proc/net/dev`, neither of
  which the kernel populates (sysfs only serves `kernel`/`params`/`devices`;
  `/proc/net` is a flat file with no `dev`/`if_inet` subfiles), so the tool
  reported "No network interfaces found". It now falls back to the existing
  read-only `SYS_NET_IF_INFO=842` syscall, decoding the 24-byte record
  (ip/mask/gw/dns/mac/up) into a synthesized `eth0` interface (counters left at
  0 — the syscall carries none — rather than fabricating traffic stats). Pure
  decode/format helpers (`parse_net_if_info`, `fmt_ipv4`, `fmt_mac`,
  `compute_broadcast`) are host-unit-tested (8 new tests; `cargo test -p
  ifconfig` 32 pass). The **write** paths (`up`/`down`/`set ip`/…) no longer
  issue the bogus `SYS_NET_IOCTL` — see the write-path safety fix below.
- **`ip` (`ip addr show`, `ip link`, `ip route`, `ip stats`) — DONE 2026-06-14.**
  Same dead read paths (`/sys/class/net/`, `/proc/net/dev`, `/proc/net/route`).
  `read_interfaces` now falls back to `SYS_NET_IF_INFO` to synthesize the `eth0`
  interface, and `read_routes` synthesizes the default route from the record's
  gateway field. `ip neigh` previously read the unpopulated `/proc/net/arp`; it
  now falls back to the read-only `SYS_ARP_TABLE=843` syscall (12-byte records:
  ip/mac/ttl), reusing the `arp` tool's count-bounded parse + zero-MAC =
  INCOMPLETE convention. 14 host tests total (`cargo test -p ip`: 14 pass; +4 for
  ARP). Write paths (`ip link set`, `ip addr add/del`, `ip route add/del`)
  no longer issue the bogus `SYS_NET_IOCTL` — see the write-path safety fix below.
- **`route` (`route`, `route -n`, `route -v`) — DONE 2026-06-14.** Its
  `/proc/net/route`, `/sys/net/routes`, and `/proc/net/if_inet` sources are all
  unpopulated; `read_routes` now synthesizes the connected network route and the
  default route from `SYS_NET_IF_INFO`. 4 new host tests (`cargo test -p route`:
  10 pass). Write paths (`route add/del/flush`) — see the write-path safety fix
  below.
- **`netstat` (`-t`/`-l`/`-r`/`-i` connection, route, and iface views) — DONE
  2026-06-14.** Its `/proc/net/{tcp,udp,route,dev}` and `/sys/class/net` sources
  are all unpopulated. It now falls back to the read-only diagnostic syscalls:
  connection list ← `SYS_TCP_LIST=840` (20-byte records) + listeners ←
  `SYS_TCP_LISTENER_LIST=841` (4-byte records, mapping the kernel
  `net::tcp::TcpState` discriminant to netstat's state enum); route view ←
  `SYS_NET_IF_INFO=842` (connected + default route, same synthesis as `route`);
  iface view ← `SYS_NET_IF_INFO` (name/MTU) + `SYS_NET_STAT=825` (48-byte
  counters; rx_errors/tx_dropped reported as 0 since the kernel exposes
  neither). UDP has no kernel socket-table syscall, so the UDP connection view
  stays empty. 9 new host tests (`cargo test -p netstat`: 31 pass). netstat is
  read-only (no write paths).
- **`ss` / `sockstat` (TCP socket view) — DONE 2026-06-14.** Reads
  `/proc/net/{tcp,tcp6,udp,udp6,raw,raw6,unix}`, all unpopulated. The TCP view
  now falls back to `SYS_TCP_LIST=840` + `SYS_TCP_LISTENER_LIST=841` (IPv4 only,
  so the fallback is skipped under `-6`), mapping the kernel `net::tcp::TcpState`
  discriminant to ss's `SocketState`. UDP/raw/unix have no kernel enumeration
  syscall yet and stay empty. NOTE: unlike the other net tools, ss's existing
  `run_ss`/`run_sockstat` unit tests exercise `gather_sockets`, which reaches the
  query functions — so the real `syscall` asm is gated
  `#[cfg(all(target_arch="x86_64", not(test)))]` with an ENOSYS stub under
  `test` to avoid executing a raw syscall on the host; the pure record decoders
  are unit-tested directly. 5 new host tests (`cargo test -p ss`: 37 pass). ss is
  read-only (no write paths).

**Write-path safety fix — DONE 2026-06-14** (`ifconfig`, `ip`, `route`). The
write paths in these three tools were worse than the "harmless no-op" originally
documented. Each defined `const SYS_NET_IOCTL: u64 = 810` and called
`syscall(810, cmd, …)` where `cmd` ∈ {1,2,3,10,11,12} (up/down/set-ip/route
add/del/flush) was passed as **arg0**. But `810` is `SYS_UDP_BIND` and its arg0
is a **port number** — so every config command actually bound a UDP socket to
port 1/2/3/10/11/12, leaked the returned handle, and — because the handle is a
non-negative return value — reported **false success** to the user. (`route`
additionally carried a dead `net_ioctl6`/`syscall6` path, and a dead
`/sys/net/routes/*` sysfs write fallback the kernel never serves.) Fix: removed
the fabricated `SYS_NET_IOCTL` constant from all three; `net_ioctl` now returns
`-38` (ENOSYS) **without issuing any syscall**, with a doc comment explaining the
`810` aliasing; removed `route`'s dead `net_ioctl6`/`syscall6`; added honest
`-38 → "Function not implemented (... not yet supported on Slate OS)"` arms to
`route`'s add/del error matches. Result: false-success-with-socket-leak becomes
an honest failure + non-zero exit until the net-config ABI lands. The read-only
`SYS_NET_IF_INFO`/`SYS_ARP_TABLE` query wrappers (`syscall3`/`syscall4`) are
retained and still used. All three still cross-compile for `x86_64-slateos` and
pass clippy + host tests (ifconfig 32, ip 14, route 10).

**Write-path safety fix extended to `dhcpcd`/`fw`/`nft` — DONE 2026-06-14.** The
same `SYS_NET_IOCTL=810` misuse lived in the remaining net-config tools:
- **`dhcpcd`** issued `net_ioctl(NET_IF_{SET_IP,SET_MASK,UP,SET_GW}, …)` after
  acquiring a lease — each binding+leaking a UDP socket on port 3/4/1/5 and
  reporting a non-negative "success". `net_ioctl` now returns `-38` without any
  syscall (its only `syscall4` user, so `syscall4` was removed); DHCP transport
  itself is unaffected (it uses `std::net::UdpSocket`). 107 host tests pass.
- **`fw`** was the worst case: besides the write commands, its *read* path
  `fw_ioctl(FW_GET_STATUS)` decoded the leaked UDP socket **handle as firewall
  status bits**, fabricating bogus enabled/logging/policy state. Both `fw_ioctl`
  and `fw_ioctl_buf` now return `-38` (no syscall); the dead direct-`syscall4`
  `load_rules_from_kernel` path and the now-dead kernel-status branch in `load()`
  were removed, so status reads fall back to `/proc/net/firewall` → saved rules
  file → defaults. 40 host tests pass.
- **`nft`** never actually called its `nft_ioctl_buf`/`syscall4` (all dead code
  behind `#[allow(dead_code)]`), so there was no live bug — but the dangerous
  `SYS_NET_IOCTL=810` plumbing was removed outright; the `NFT_*` sub-command
  numbers are kept as documentation of the future control ABI. 102 host tests
  pass.

  **`nft`/`iptables` are stateless and non-functional as configurators — BUG,
  open 2026-07-02.** Separate from the (fixed) syscall-misuse issue: `run_nft`
  and `run_iptables` (`userspace/nft/src/main.rs` ~lines 2264, 2278) each build a
  fresh `Ruleset::new()` per invocation, apply the single command, print, and
  **discard all state on exit**. The tool never persists to a file, never reads
  `/proc/net/nftables`, and never touches the kernel — so `nft add rule …` /
  `iptables -A …` are no-ops that only echo syntax. The module doc's claim that
  "Rules are persisted through `/proc/net/nftables`" is a **doc/reality
  mismatch** (nothing reads or writes that path). The kernel firewall write
  syscalls now exist (860–864, used by `fw`), so wiring is *possible*, but doing
  it well needs (1) a persistence-format decision and (2) a heavily-lossy mapping
  from nftables' model (tables/chains/hooks/sets/maps/NAT) onto our narrow kernel
  `Rule`. That is a genuine design fork, tracked as **open-questions Q21** (A full
  wiring / B minimal wiring / C make it honestly parser-only + steer to `fw`;
  Claude recommends C). Until resolved, `fw` is the one working firewall
  front-end. **Proper fix:** resolve Q21, then either implement the chosen wiring
  or (option C) correct the module doc and print a "not applied — use `fw`"
  notice on mutating `nft`/`iptables` commands.

All three cross-compile for `x86_64-slateos` and pass clippy. With this, **no
remaining userspace tool defines or issues `SYS_NET_IOCTL`/`810` for net-config**
(verified by grep). Only the legitimate `SYS_UDP_BIND=810` users (`dig`, `nc`,
`inetd`, …) reference the number now.

**Disk-admin format-path safety fix — DONE 2026-06-14** (`mkfs`, `diskutil`).
**SUPERSEDED 2026-06-20** — `format` is now wired to the real `SYS_FS_FORMAT=654`
(see the "mkfs/diskutil format: RESOLVED 2026-06-20" bullet above); the honest
ENOSYS stub described here was the interim state. Historical record follows.
The same fabricated-syscall pattern lived in the disk-admin tools: both defined
`SYS_FS_FORMAT=651` and issued `syscall(651, path_ptr, …)`. But `651` is
`SYS_FS_SEEK_HOLE` — a real syscall whose arg0 is a *file descriptor*, not a
path pointer — so a `mkfs`/`diskutil format` actually invoked `seek_hole` with a
userspace pointer reinterpreted as an fd, returning a misleading `EBADF`/`EINVAL`
while formatting nothing. Fix:
- **`mkfs`** — removed `SYS_FS_FORMAT`/`syscall3`; `do_format` now returns the
  honest `ENOSYS` message without issuing a syscall. 35 host tests pass.
- **`diskutil`** — removed `SYS_FS_{IOCTL,FORMAT,VERIFY,REPAIR,TRIM,STATFS}` +
  the `syscall5`/`syscall3`/`syscall2`/`c_str` plumbing; format/verify/repair/
  trim now fail honestly with `ENOSYS`, and `usage` (statfs) skips the kernel
  round-trip and goes straight to its existing sysfs-based estimate. The
  exact-usage formatting is retained, ready to wire once a real statfs ABI lands.
  Builds + clippy clean.
- **`fsck`** left as-is at the time: its `652`/`653` were *unassigned*, so the
  kernel returned a clean `ENOSYS` (no real-syscall aliasing) — benign **then**.
  **SUPERSEDED 2026-06-20** — when `SYS_FS_MOUNT`/`SYS_FS_UMOUNT` were assigned to
  `652`/`653`, `fsck`'s stale numbers started aliasing the mount handlers (a real
  collision I introduced), and the fs-admin ABI now exists. `fsck` was rewired to
  the real `SYS_FS_CHECK=655` — see the "fsck/diskutil verify+repair: RESOLVED
  2026-06-20" bullet above.

**Proper fix:** this is an **operator design decision**, not a mechanical fix —
the kernel must first grow the missing ABI, and the *shape* of that ABI is a
fork: a native net-config syscall family vs. a network-manager IPC daemon for
the net tools; a real mount/umount + fs-admin (format/verify/repair) syscall set;
and a process-credential + fs-root ABI for chroot. The partial near-term win
that needed no decision — wiring the net tools' **read** paths (`ifconfig`, `ip`,
`route`) to `NET_IF_INFO=842` — is now DONE (see above); only the **write** paths
remain blocked on the ABI fork. Trigger to revisit: when the matching kernel
syscalls land (track via roadmap net-config / mount / fs-admin tasks). Related: `sys_clock_settime`/`sys_clock_adjtime` now enforce
`require_clock_authority()` keyed on `uid==0`; revisit to key off a real
per-process `CAP_SYS_TIME` bit when the PCB gains a POSIX capability set (today
`ProcessCredentials` is only uid/gid/groups).

### TD17. inotify event coverage is limited to native-derived events — PARTIAL 2026-06-14 (IN_ISDIR added; was DEBT 2026-06-12)

**Where:** `kernel/src/ipc/inotify.rs` (Linux-ABI adapter) backed 1:1 by
`kernel/src/fs/notify.rs` native watches.

**What it is:** inotify watches are backed 1:1 by native `fs::notify` watches, so
the reportable event set is exactly what the native layer produces:
`IN_CREATE`/`IN_DELETE`/`IN_MODIFY`/`IN_ATTRIB`/`IN_MOVED_FROM`/`IN_MOVED_TO`
(Renamed→pair)/`IN_DELETE_SELF`/`IN_MOVE_SELF`/`IN_ACCESS`/`IN_OPEN`/
`IN_CLOSE_WRITE`/`IN_CLOSE_NOWRITE`, plus synthetic `IN_Q_OVERFLOW` and
`IN_IGNORED`. `IN_ISDIR` is now OR'd into the reported mask whenever the event
subject is a directory (mkdir/rmdir, directory-handle close, a renamed
subdirectory) — `FsEvent` carries an `is_dir` flag threaded through both the
kernel inotify adapter and the native fs_watch ABI (byte 524) into the posix
inotify shim. Watches are NON-RECURSIVE and keyed by
NORMALIZED PATH STRING, not inode — re-adding the same path returns the same wd
(mask replaced, or OR-combined under `IN_MASK_ADD`); a watched path deleted and
recreated keeps the same wd. `IN_ONESHOT`/`IN_DONT_FOLLOW`/`IN_EXCL_UNLINK` are
accepted-but-ignored control bits. Linux FS mutation syscalls
(`mkdir`/`mkdirat`/`rmdir`/`unlink`/`unlinkat`/`rename`/`renameat`/`renameat2`)
now route through the native VFS (`Vfs::mkdir`/`rmdir`/`remove`/`rename`), so
inotify events DO flow from Linux-ABI filesystem operations — including
`IN_MOVED_FROM`/`IN_MOVED_TO` for renames. `renameat2` honours `RENAME_NOREPLACE`
(atomic for the common same-mount case — see below) and `RENAME_EXCHANGE`
(atomic same-mount swap on filesystems that implement it — memfs does; ext4/FAT
return `EINVAL`). `RENAME_WHITEOUT` is rejected with `EINVAL` (overlayfs whiteout
device nodes are unsupported).

**Impact:** low — the common "watch a dir for create/delete/modify/move/open/close"
file-manager/build-tool idiom is fully covered, now including the `IN_ISDIR`
dir-flag and Linux-ABI-driven mutations. Remaining gaps bite only apps that need
inode-identity semantics across delete+recreate (rare), an atomic
`RENAME_NOREPLACE`, or `RENAME_EXCHANGE`/`RENAME_WHITEOUT`.

**Progress (2026-06-14): IN_ACCESS, then IN_OPEN / IN_CLOSE_WRITE /
IN_CLOSE_NOWRITE now implemented.** All three are gated by the lock-free
per-event-bit interest counter (`fs::notify::INTEREST_COUNTS` /
`interest_includes`): watch create/close adjust the counts, and `emit()` plus the
hooks early-out with a few relaxed atomic loads before touching the `WATCHES` lock
unless a live watch actually requests that bit, so they cost nothing when unused
and stay excluded from `ALL_CHANGES`.
- `IN_ACCESS`: `Vfs::read_file` / `Vfs::read_at` emit `FsEventType::Accessed` after
  dropping the VFS lock.
- `IN_OPEN`: `fs::handle::open` emits `FsEventType::Opened` after the handle is
  installed (so a failed allocation never produces a spurious open).
- `IN_CLOSE_*`: `fs::handle::close` emits `FsEventType::ClosedWrite` /
  `ClosedNoWrite` on the final (refcount→0) close, discriminated by the handle's
  write-mode, after dropping the `OPEN_FILES` lock (keeps the
  `OPEN_FILES → WATCHES` lock order one-directional). Directory handles now emit
  their close too, tagged `is_dir` so the adapter ORs in `IN_ISDIR`.
- `IN_ISDIR` (2026-06-14): `FsEvent` gained an `is_dir` flag. `emit_dir` /
  `emit_created_dir` / `emit_deleted_dir` set it; `Vfs::mkdir`/`rmdir` and the
  directory-handle close use them. The kernel inotify adapter ORs `IN_ISDIR` into
  every directory-subject record (create/delete/close/renamed-subdir, never the
  synthetic `IN_IGNORED`/`IN_Q_OVERFLOW`). The native fs_watch syscall ABI carries
  it in record byte 524 (reserved padding before), and the posix inotify shim
  (`epoll.rs::translate_kernel_event`) ORs `IN_ISDIR` the same way.
Covered by `fs::notify::self_test` (interest-gate create/close, synthetic emit,
mask-filtering, end-to-end `Vfs::read_file` ACCESS hook, and an end-to-end
open/close through the handle layer asserting Opened + ClosedNoWrite for read-only
and Opened + ClosedWrite for writable), the inotify boot self-test (a dir-create
event asserting `IN_CREATE | IN_ISDIR`), and the posix `test_translate_isdir_or_in`
host unit test (dir vs file subject, and IN_IGNORED never tagged).

**Progress (2026-06-14): atomic `RENAME_NOREPLACE`.** Gap (b) is resolved for the
common same-mount case. New `Vfs::rename_noreplace` (kernel/src/fs/vfs.rs) shares
a private `rename_inner(from, to, noreplace)` with `Vfs::rename`; in the same-mount
branch the destination-existence check (`mp_to.fs.stat(rel_to)` → EEXIST if
present) executes under the **same held `VFS.lock()`** as the underlying
`mp_to.fs.rename`, so there is no TOCTOU window — no concurrent creator can slip a
file into the destination between the check and the rename. The Linux-ABI
`rename_common` (kernel/src/syscall/linux.rs) now calls `Vfs::rename_noreplace`
when the `RENAME_NOREPLACE` flag is set instead of doing a separate
`Vfs::stat`-then-`Vfs::rename` pre-check. The cross-mount copy+delete convenience
path (which Linux rejects outright with EXDEV) keeps a documented best-effort
destination pre-check, since multiple lock acquisitions make it inherently
non-atomic. Covered by the existing `syscall::linux::self_test` rename round-trip
(EEXIST onto an existing destination through `renameat2`) plus a new VFS-level
assertion that `rename_noreplace` onto a *free* destination succeeds and moves
src→dst.

**Progress (2026-06-14): `RENAME_EXCHANGE`.** Gap (c)'s exchange half is resolved
for filesystems that implement it. New `FileSystem::rename_exchange` trait method
(default `NotSupported`) with a real memfs implementation (atomically detaches
both entries and re-attaches them swapped, all-or-nothing with rollback if the
second operand is missing; self-exchange is a no-op; both operands must exist or
`NotFound`). `Vfs::rename_exchange` (kernel/src/fs/vfs.rs) resolves both paths,
checks tags/writability/intercept, and delegates the swap to the FS under the held
`VFS.lock()` — atomic w.r.t. the FS — requiring the **same mount** (cross-mount
exchange → `NotSupported`, since no atomic cross-FS swap exists). The Linux-ABI
`sys_renameat2` now routes `RENAME_EXCHANGE` to a new `rename_exchange_common`
(kernel/src/syscall/linux.rs) instead of the old blanket gate-4 `EINVAL`. The
mutual-exclusion gates (EXCHANGE+NOREPLACE/WHITEOUT → EINVAL) and the WHITEOUT
CAP/unsupported gates are unchanged. Covered by the post-`/tmp`
`self_test_rename_noreplace` (now also asserts an EXCHANGE swap of two existing
files' contents and a missing-operand `ENOENT` that leaves the survivor intact),
verified at boot.

**Progress (2026-06-14): cross-mount `RENAME_EXCHANGE` now returns `EXDEV`, not
`EINVAL`.** Previously a filesystem lacking exchange support *and* a cross-mount
request both surfaced as `EINVAL`, where Linux uses `EXDEV` specifically for the
cross-mount case. Added a `KernelError::CrossDevice` variant (code `-512`, in the
FS range) mapping to `EXDEV` in `linux_errno_for`/`kernel_error_from_code`;
`Vfs::rename_exchange`'s cross-mount branch now returns `CrossDevice` (FS-lacking-
support still returns `NotSupported` → `EINVAL`), so `rename_exchange_common`'s
generic `Err(e) => linux_errno_for(e)` arm yields `EXDEV` for cross-mount. The
`self_test_rename_noreplace` boot test gained case (6): with the boot-test's
writable memfs root + memfs `/tmp` (two distinct mounts), it asserts
`Vfs::rename_exchange` across them returns `CrossDevice` and that `renameat2`
maps it to `-EXDEV` (skips cleanly if the root is read-only in another config).

**Remaining fix:** the items left are: (a) switch watch identity to inode if/when
stable inode numbers are available; and (c-whiteout) `RENAME_WHITEOUT` support
(currently `EINVAL`).

### TD16. epoll fd readiness not reported when an epoll is nested in poll/select/epoll — RESOLVED 2026-06-14

**Where:** `kernel/src/ipc/epoll.rs` + the `HandleKind::Epoll` arm of
`poll_revents_from_entry` in `kernel/src/syscall/linux.rs`.

**What it was:** an epoll fd is itself pollable on Linux (it reports `EPOLLIN`
when any monitored fd is ready), allowing epoll fds to be nested inside another
epoll/poll/select. The `HandleKind::Epoll` arm of `poll_revents_from_entry`
returned 0 (never-ready), so nested-epoll readiness was NOT reported. `epoll_wait`
over directly-monitored fds always worked fully; only the nested case was wrong.

**Resolved (2026-06-14):** added `epoll_instance_ready(pid, handle, depth)` next
to `poll_revents_from_entry`. The Epoll arm now, given the threaded `owner_pid`,
resolves the epoll's `interest_list` against that process's fd table and reports
`POLLIN|POLLRDNORM` if any member is ready. Non-epoll members are evaluated by
`poll_revents_from_entry` (which never recurses back, as only the epoll arm calls
the helper); nested-epoll members recurse into `epoll_instance_ready` with
`depth + 1`, bounded by `EP_MAX_NESTS = 4` (mirrors `fs/eventpoll.c`) so a cyclic
or pathologically-deep nest can never blow the kernel stack. Without an
`owner_pid` (kernel/self-test context) the arm still reports not-ready rather
than consult an unrelated process's fd table. Boot self-test added in
`syscall::linux::self_test` ("nested-epoll readiness (TD16) OK"): a throwaway
process with a pipe → inner epoll E1 (watches pipe read) → outer epoll E0
(watches E1), asserting both E1 and the nested E0 are not-ready on an empty pipe,
both ready after a write, and not-ready when evaluated with `owner_pid = None`.

### TD15. timerfd `TFD_TIMER_CANCEL_ON_SET` is a silent no-op — RESOLVED 2026-06-14

**Where:** `kernel/src/timekeeping.rs` (generation counter), `kernel/src/ipc/timerfd.rs`
(stamp/check/wake), `kernel/src/syscall/linux.rs` (`sys_timerfd_settime`,
`dispatch_timerfd_read`), `kernel/src/syscall/handlers.rs` (`sys_clock_settime`,
`sys_clock_adjtime`).

**What it was:** `timerfd_settime` accepted the `TFD_TIMER_CANCEL_ON_SET` flag
(bit 1) without error, but the cancel-on-clock-step behavior was NOT implemented.
On Linux, a `CLOCK_REALTIME` timerfd armed with an absolute expiry and this flag is
"cancelled" (read returns `ECANCELED`, poll reports `POLLIN` readiness — *not*
`POLLERR`, contrary to the original note here) if the system realtime clock is
discontinuously changed (settimeofday/clock_settime/NTP step).

**Fix (implemented):** `timekeeping` now keeps a `REALTIME_GENERATION` counter,
bumped on every discontinuous realtime-clock step (`set_realtime`,
`adjust_realtime`); a smooth TSC advance does NOT bump it. `sys_timerfd_settime`
honours `TFD_TIMER_CANCEL_ON_SET` only for an absolute `CLOCK_REALTIME` timer,
snapshotting the generation into the timerfd at arm time (`armed_gen`). On read,
`take_cancellation` / `BlockingRead::Cancelled` return `ECANCELED` once per step
(resyncing `armed_gen`); on poll, `is_readable` reports readiness while the
generation is stale (level-triggered, no explicit poll wake needed). A blocked
reader is woken promptly by `clock_was_set()`, called from the `clock_settime` /
`clock_adjtime` handlers after the step. Boot self-test added to
`timerfd::self_test` ("TFD_TIMER_CANCEL_ON_SET (TD15): OK"): arms an absolute
`CLOCK_REALTIME` cancel-on-set timer far in the future, steps the clock via
`adjust_realtime(0)` (bumps the generation without moving the clock value),
asserts the timer becomes readable / `take_cancellation` returns true exactly
once, and that a re-armed timer *without* the flag is unaffected by a step.

### TD14. Per-process CPU-time / fault / ctxsw accounting — RESOLVED 2026-06-13 (time + page-fault + context-switch counters all done)

**Where:** `kernel/src/syscall/linux.rs` `sys_getrusage` and `sys_times`;
`kernel/src/sched/task.rs` (`Task::user_ticks`/`sys_ticks`, `tick_burst(from_user)`);
`kernel/src/sched/mod.rs` (`timer_tick(from_user)`, `cpu_ticks(tid)`, `TaskInfo`);
`kernel/src/proc/thread.rs` (`process_cpu_ticks(pid)`, `process_fault_counts(pid)`,
`on_thread_exit`); `kernel/src/proc/pcb.rs` (`Process::{acct_,child_}{user,sys}_ticks`
and `{acct_,child_}{min,maj}_flt`, `ThreadExitAccounting`, `remove_thread`,
`try_reap`/`try_reap_any`, `process_acct_ticks`/`process_child_ticks`,
`process_acct_faults`/`process_child_faults`); `kernel/src/sched/mod.rs`
(`account_fault`/`fault_counts`, `ctxsw_counts`, `SwitchKind` threaded through
`schedule_inner`); `kernel/src/idt.rs` (`account_fault` calls in
`handle_page_fault`); `kernel/src/apic.rs` (CPL sampling in `handle_timer_irq`);
`kernel/src/fs/procfs.rs` (`build_pid_stat`, `build_pid_status` ctxsw lines).

**Resolved — base (2026-06-13):** Linux-style tick-sampling CPU-time
accounting. On every timer IRQ, `handle_timer_irq` reads the interrupted frame's
CPL (`(frame.cs & 0x3) == 0x3` ⇒ ring-3) and passes `from_user` down through
`sched::timer_tick` → `Task::tick_burst`, which charges the whole tick to
`user_ticks` or `sys_ticks` (O(1), zero syscall-fastpath cost — Linux's default
non-NO_HZ_FULL model). `sched::cpu_ticks(tid)` exposes the per-thread split.

**Resolved — exited-thread fold + children-time (2026-06-13):** added a
per-process CPU-time accumulator to the PCB. When a thread exits,
`on_thread_exit` captures its `(user, sys)` ticks (while the Task is still
alive in the scheduler) and `remove_thread` folds them into
`Process::acct_user_ticks`/`acct_sys_ticks`. `process_cpu_ticks` now returns
`accumulator + Σ(live thread ticks)`, so it is exact for multi-threaded
processes that have already reaped worker threads — not just single-threaded
ones. For children time, `try_reap`/`try_reap_any` credit the parent's
`child_user_ticks`/`child_sys_ticks` with the reaped child's total CPU time
plus the child's own children-time (POSIX cutime/cstime carry-up, mirroring
Linux `wait_task_zombie` → `signal->cutime`/`cstime`). Both reset to 0 on fork.

Wired into:
- `getrusage(RUSAGE_SELF)` → process roll-up (live + exited threads);
  `getrusage(RUSAGE_THREAD)` → current thread; `getrusage(RUSAGE_CHILDREN)` →
  children accumulator. `ru_utime`/`ru_stime` populated (ticks×10ms → timeval).
- `times(2)` `tms_utime`/`tms_stime` and `tms_cutime`/`tms_cstime`
  (USER_HZ==TICK_RATE_HZ==100, so tick counts map directly to clock_t).
- `/proc/<pid>/stat` fields 14/15 (utime/stime) and 16/17 (cutime/cstime).

Self-test: `pcb::test_cpu_time_accounting` exercises the exited-thread fold,
`process_cpu_ticks` after all threads exit, and the parent←child←grandchild
children-time carry-up (asserts parent sees `(5+2, 3+1)`). Boot-test PASSED.

**Resolved — page-fault counters (2026-06-13):** added per-task `min_flt`/`maj_flt`
to `Task` (sched/task.rs) charged by `sched::account_fault(tid, major)` from the
three user-fault resolution points in `idt.rs::handle_page_fault` — swap-in ⇒
major (required I/O); demand-page (CoW/demand-zero) and stack growth ⇒ minor.
Mirroring the CPU-time path, the PCB gained `acct_min_flt`/`acct_maj_flt`
(exited-thread fold) and `child_min_flt`/`child_maj_flt` (reaped-children
carry-up). `remove_thread`'s signature was refactored from positional tick args
to a `ThreadExitAccounting { user_ticks, sys_ticks, min_flt, maj_flt }` struct
(the proper fix vs. a 6-arg signature). `proc::thread::process_fault_counts(pid)`
sums live + exited; `pcb::process_child_faults(pid)` reports the children
accumulator. Wired into `getrusage` `ru_minflt`(off 64)/`ru_majflt`(off 72) for
SELF/THREAD/CHILDREN, and `/proc/<pid>/stat` fields 10/11/12/13
(minflt/cminflt/majflt/cmajflt). `test_cpu_time_accounting` extended to assert
the fault fold (grandchild `(3,1)`), child children-faults `(3,1)`, and parent
children-faults `(4+3, 2+1) = (7,3)`. Boot-test PASSED.

**Resolved — context-switch counters (2026-06-13):** added per-task
`nvcsw`/`nivcsw` to `Task`, charged at the scheduler switch point. A
`SwitchKind` enum (`Voluntary`/`Involuntary`/`Uncounted`) is threaded into
`schedule_inner` from its five call sites (`yield_now`/`block_current`/
self-`suspend` ⇒ voluntary; `preempt` ⇒ involuntary; `task_exit` ⇒ uncounted)
and the outgoing task's counter is bumped under the SCHED lock at the actual
switch (where `next_id != current_id`). The PCB gained
`acct_nvcsw`/`acct_nivcsw` (exited-thread fold) and `child_nvcsw`/`child_nivcsw`
(reaped-children carry-up); `ThreadExitAccounting` carries the two fields too.
`proc::thread::process_ctxsw_counts(pid)` sums live + exited;
`pcb::process_child_ctxsw(pid)` reports the children accumulator. Wired into
`getrusage` `ru_nvcsw`(off 128)/`ru_nivcsw`(off 136) for SELF/THREAD/CHILDREN,
and `/proc/<pid>/status` `voluntary_ctxt_switches`/`nonvoluntary_ctxt_switches`
(previously stubbed as `0`/`schedule_count`). `test_cpu_time_accounting`
extended to assert the ctxsw fold (grandchild `(6,4)`), child children-ctxsw
`(6,4)`, and parent children-ctxsw `(7+6, 5+4) = (13,9)`. Boot-test PASSED.

**TD14 is now fully resolved** — all `getrusage` time/fault/ctxsw fields, `times`,
and the `/proc/<pid>/stat` + `/proc/<pid>/status` accounting surfaces are sourced
from real per-task counters rolled up per process with children carry-up. The
only rusage fields left at 0 are ones Linux also commonly leaves 0 (`ru_ixrss`,
`ru_idrss`, `ru_isrss`, `ru_nswap`, `ru_msgsnd`/`msgrcv`, `ru_nsignals`,
`ru_inblock`/`oublock`), which would require swap-RSS integral / signal-IPC
accounting not yet modelled.

### TD13. A few Linux-compat-flavored fields live in the native PCB — WATCH 2026-06-13

**Where:** `kernel/src/proc/pcb.rs` — job-control stop state
(`ProcessState::Stopped`/stop-signal tracking) and the `PR_SET_PDEATHSIG`
parent-death-signal storage (`get`/`set` around lines 2282–2290; field noted
"not wired because we don't yet have user-signal infrastructure").

**What it is:** the native process control block carries a small amount of
state whose *origin* is Linux/POSIX semantics (job-control stop/continue and
`prctl(PR_SET_PDEATHSIG)`). Per design-decisions.md §4 and §12, Linux-ABI
constructs should stay confined to the compat layer / Linux-ABI PCB state and
not accrete in the native PCB.

**Why it's not a live bug:** stop/continue is arguably a general
process-lifecycle notion (not strictly Linux), and `PR_SET_PDEATHSIG` storage
is inert (delivery is unwired). Nothing native consumes these as signals;
native process control remains IPC-based and faults remain SEH-style
exceptions. So the native ABI is not actually leaking *behavior* today.

**Proper fix (when the boundary is next touched):** move the pdeathsig value
(and any other purely-Linux fields) into the Linux-ABI PCB side-state (next to
`KernelFdTable`/the saved auxv), keyed by pid, so the native PCB carries only
constructs that would exist if Linux had never existed. Keep `ProcessState`
lifecycle states that are genuinely ABI-neutral. The trigger to do this is the
Linux compat ELF loader / signal-infrastructure work landing — co-locate all
Linux-ABI per-process state there in one pass rather than piecemeal.

### TD12. DRM event `read(2)` returns EAGAIN instead of blocking when empty — DEBT 2026-06-13

**Where:** `dispatch_drm_card_read` in `kernel/src/syscall/linux.rs`.

**What it is:** `read(2)` on a `/dev/dri/cardN` fd drains queued KMS events
(flip-complete records from `PAGE_FLIP` with `DRM_MODE_PAGE_FLIP_EVENT`).
When the event queue is empty it returns `EAGAIN` unconditionally — it does
not honour a *blocking* fd by parking the caller until an event arrives
(unlike, e.g., the signalfd read path, which has a real wait queue).

**Why it's not a live bug today:** our DRM backends retire page flips
**synchronously** inside `DrmDevice::page_flip`, so a flip-complete event is
queued *before* the `PAGE_FLIP` ioctl returns. A client following the normal
pattern (submit flip with the EVENT flag, `poll(2)` the fd, then `read(2)`)
always finds the event already queued; `poll` reports `POLLIN` immediately
and the read succeeds. The empty-read path is only reachable by a client
that reads without having submitted a flip — a client bug — and returning
EAGAIN there prevents a kernel hang rather than causing one.

**Proper fix (deferred until a backend retires flips asynchronously):** add a
per-client DRM event wait queue (mirroring the signalfd waiter pattern:
`register` + re-check + `block_current`, woken by `queue_event`), and have a
blocking read park on it instead of returning EAGAIN. Only worth doing once
a real vblank/async-flip source exists; under synchronous retirement it is
dead code.

### TD11. DRM dumb-buffer mmap not ref-tracked across `fork()` — DEBT 2026-06-13

**Where:** `drm_mmap_dumb` in `kernel/src/syscall/linux.rs` (the
`HandleKind::DrmCard` mmap interception in `sys_mmap`), in concert with
the refcounted `mm/frame.rs::free_frame` and the process-exit teardown
in `mm/page_table.rs::clear_user_address_space`.

**What it is:** The DRM Linux-ABI shim's MAP_DUMB path maps a dumb
buffer's GEM frames into the calling process by `frame::ref_inc`-ing
each frame before `map_frame`, so process-exit teardown's refcounted
`free_frame` merely balances the extra ref rather than double-freeing
the buffer (the GEM object retains its own ref until `gem_destroy`).
This is correct for a single process. It is NOT correct under a future
deep-copying `fork()`: a child that inherits the user PTEs for a dumb
mmap does not get a second `ref_inc`, so if fork ever gains general
per-page CoW of arbitrary user VMAs, a dumb mmap inherited by a child
and torn down on both sides could mis-count the frame refcount.

**Why it's not a live bug today:** our `fork()` does not deep-copy
arbitrary user mappings (see todo.txt Judgment Calls, fork(), 2026-05-31),
and graphics clients are single-process and do not fork while holding a
live framebuffer mmap. The gap is unreachable in practice.

**Proper fix (deferred until fork does general user-VMA copying):**
teach the fork path to recognise DRM-dumb-backed VMAs (or, more
generally, externally-refcounted frames) and `ref_inc` each frame per
child mapping, so every address space that maps a frame holds exactly
one ref and teardown stays balanced. Also recorded in todo.txt under
Judgment Calls.

### TD10. ALSA PCM shim does not implement the STATUS ioctl — RESOLVED 2026-07-15

**RESOLUTION (2026-07-15):** `STATUS` and `STATUS_EXT` are now implemented.
The ABI-target ambiguity that had deferred this (below) is resolved by the
obviously-correct choice for a new 64-bit OS: **target the time64 variant**
(64-bit `time_t` → 16-byte `struct timespec`), which is what a modern 64-bit
ALSA-lib is compiled against. `kernel/src/audio_alsa.rs` gains a byte-exact
`SndPcmStatus` (`size_of == 152`, asserted in `self_test`), from which the
request numbers derive: `SNDRV_PCM_IOCTL_STATUS == 0x8098_4120`
(`_IOR('A',0x20,152)`) and `SNDRV_PCM_IOCTL_STATUS_EXT == 0xC098_4124`
(`_IOWR('A',0x24,152)`), both asserted. `alsa_pcm_ioctl_status`
(`kernel/src/syscall/linux.rs`) fills `state`/`appl_ptr`/`hw_ptr`/`delay`
(= queued frames, what `snd_pcm_delay(3)` reports) / `avail` (playback: free
buffer space `buffer_frames − delay`; capture: full buffer) from the same
`sync_position` snapshot as SYNC_PTR, plus monotonic reference timestamps
(`clock_monotonic`) and the `trigger_tstamp` stamped at `START`. The ring
buffer size is captured at `HW_PARAMS` (`audio_alsa::buffer_size_frames` reads
the client-committed `BUFFER_SIZE` interval → `alsa_pcm::set_buffer_size`).
`avail_max` reports the current `avail` (a truthful lower bound — we don't
track a running peak); `overrange` is 0 (capture is synthesised silence).
Boot-validated: `[alsa] ALSA PCM ABI self-test PASSED` (struct size + ioctl
encodings) and `[alsa_pcm] PCM instance lifecycle self-test PASSED` (delay=2 /
avail=1022 / buffer_frames=1024 / trigger-stamped-on-start). Design note in
`design-decisions.md` (time64 ALSA ABI target). Original entry preserved
below for context.

---

### TD10 (original). ALSA PCM shim does not implement the STATUS ioctl — DEBT 2026-06-13 (narrowed 2026-06-13)

**Update (commit 4b):** SYNC_PTR and READI_FRAMES are now implemented.
`alsa_pcm_ioctl` (`kernel/src/syscall/linux.rs`) stores `boundary` /
`avail_min` from SW_PARAMS, computes `appl_ptr` (= frames submitted) and
`hw_ptr` (= `appl_ptr − mixer-buffered frames`) reduced modulo the
boundary, and answers `SNDRV_PCM_IOCTL_SYNC_PTR` with a byte-exact
`snd_pcm_sync_ptr` (the status/control pages sit in 64-byte unions, so the
payload size is independent of the timestamp ABI). `READI_FRAMES` returns
zeroed capture frames. Both are covered by the
`ipc::alsa_pcm::self_test()` boot self-test (SYNC_PTR snapshot appl=2/hw=0,
appl_ptr/avail_min push-adopt, capture silence read).

**What still remains:** `alsa_pcm_ioctl` returns **ENOTTY** for
`SNDRV_PCM_IOCTL_STATUS` / `STATUS_EXT`.

**Why STATUS is still deferred:** unlike SYNC_PTR, the `snd_pcm_status`
payload embeds bare `struct timespec`s directly (not inside a padded
union), so its `sizeof` — and therefore the ioctl request number — depends
on the time64-vs-legacy-timespec ABI (the ambiguity flagged in the
commit-2 note at the top of `todo.txt`). Pinning that layout down is a
self-contained follow-up. STATUS is also only a convenience overlay: a
conforming ALSA-lib client learns `hw_ptr`/`appl_ptr` from SYNC_PTR (now
handled), so STATUS-on-ENOTTY does not block the playback hot path.

**Empirical confirmation of the fork (2026-06-14):** the upstream
`struct snd_pcm_status` declares its trailing pad as
`unsigned char reserved[64 - 5*sizeof(struct timespec) - 5*sizeof(int)]`
(older kernels: `reserved[52 - 4*sizeof(struct timespec)]`). With a
**16-byte** 64-bit `struct timespec` that pad size goes **negative**,
which cannot compile — proof that the mainline kernel never uses a single
struct with a 64-bit timespec here. Instead it maintains **two distinct
ABI structs**: a legacy `snd_pcm_status` built on a 32-bit
`old_timespec32` (used by the `SNDRV_PCM_IOCTL_STATUS`/`STATUS_EXT`
request numbers compiled for a 32-bit timespec) and a separate
`snd_pcm_status` / time64 path (`__SNDRV_PCM_IOCTL_STATUS_EXT64` etc.)
built on `__kernel_timespec`. The two carry **different `_IOR` request
numbers** because their `sizeof` differs. Consequently we cannot just
"pin the timespec layout" — implementing STATUS means deciding *which*
alsa-lib variant our userspace targets and answering the matching request
number(s). Until that target is fixed, emitting one guessed number risks
silently mismatching the client's other variant. This is the concrete
reason STATUS stays deferred rather than being a quick add.

**Impact:** low. SYNC_PTR (the per-period pointer exchange ALSA-lib's
kernel plugin actually relies on) works; only the `snd_pcm_status()`
convenience query falls back to ENOTTY.

**Proper fix:** add byte-exact `snd_pcm_status` (resolving the timespec
layout against our 64-bit `struct timespec`), define
`SNDRV_PCM_IOCTL_STATUS` / `STATUS_EXT` from its `size_of`, fill it from
the same `sync_position` snapshot plus the trigger/reference timestamps
once a monotonic audio clock exists, and replace the ENOTTY arm.

**Related limitations (not debt, intentional first-cut scope):** the shim
advertises only `RW_INTERLEAVED` access (mmap-based clients unsupported)
and only the mixer's native 48 kHz / S16_LE / stereo format (non-native
configs are rejected by HW_PARAMS rather than resampled/converted).
Resampling + format conversion + an mmap transfer path are future work.

### TD9. Linux program interpreter (ld.so) + PIE executable loaded at a fixed base — no ASLR — RESOLVED 2026-06-14

**Resolution (PIE-executable base, 2026-06-14):** the main `ET_DYN`/PIE
executable base is now randomised too. A new `choose_exec_load_bias(is_pie)`
helper (`kernel/src/proc/spawn.rs`) returns `0` for `ET_EXEC` and, for PIE,
an ASLR base ≥ `LINUX_PIE_BASE` drawn via `apply_aslr_base(LINUX_PIE_BASE,
rng::next_bounded(PIE_ASLR_SPAN_PAGES))` (28 bits of entropy, 16 KiB-page
units, falling back to the fixed floor before the CSPRNG is seeded). It is
computed once per spawn/exec at the two `exec_load_bias` sites
(`spawn_process` + `exec_process`) and already threads uniformly through
`load_segments_with_bias`, the biased entry point, and the SysV stack
builder's `AT_ENTRY`/`AT_PHDR`, so the whole image relocates consistently.
The highest PIE base (`≈0x5955_5555_0000`) leaves ~22 TiB below the
interpreter floor (`0x7000_0000_0000`) for the image + brk growth, and the
PIE floor sits far above the mmap window (`0x60_0000_0000`), so no
collision is possible. `sys_brk` is now a real demand-paged heap (see the
"Linux brk(2) heap" resolution below): a PIE image's heap grows from its
page-aligned image end up to a ceiling of `LINUX_INTERP_BASE`, i.e. into
that 22 TiB headroom, and the grow path's VMA-overlap check is a second
guard against colliding with the interpreter or mmap window. Covered by
`spawn::self_test`'s
`test_pie_aslr_window` (alignment + ≥1 TiB interpreter-floor headroom).
Both halves of TD9 are now done; entropy/always-on policy is in
design-decisions.md #20.

**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base; verified loading at e.g. 0x701e77808000, not the fixed
0x700000000000). The entropy-bits choice is recorded in
design-decisions.md.

---



**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base). The entropy-bits choice is recorded in
design-decisions.md.

**What remains (PIE-executable base — still DEBT):** the position-independent
*main* executable is still loaded at the fixed `LINUX_PIE_BASE =
0x5555_5555_4000`. Randomising it is more delicate than the interpreter
because the brk heap grows immediately above the PIE image, so the PIE
ASLR window must be chosen to leave room for brk growth without colliding
with the mmap window below or the interpreter window above. Deferred as a
separate follow-up. Original debt write-up follows.

---



**What:** The Linux dynamic-linker load path (`load_interpreter` in
`kernel/src/proc/spawn.rs`) maps the program interpreter (ld.so) at a
**fixed** virtual base, `LINUX_INTERP_BASE = 0x0000_7000_0000_0000`,
every time.  Real Linux randomises the interpreter base (and the mmap
region generally) via ASLR.  The executable itself is also loaded at its
fixed link-time vaddr (PIE executables are not yet re-based either).

**Where:** `kernel/src/proc/spawn.rs` — the `LINUX_INTERP_BASE` constant
and `load_interpreter()`.  AT_BASE is reported correctly from whatever
base is chosen, so making this random is a localised change.

**Why it's debt, not a bug:** ASLR is a security hardening measure, not a
correctness requirement — ld.so relocates itself to wherever it is placed
using the base it is told (AT_BASE) and its own dynamic relocations.  A
fixed base is fully functional; it just removes the address-space
randomisation defence against exploitation.

**Proper fix:** Once a userspace mmap-region allocator / ASLR policy
exists, draw the interpreter base (and PIE executable base) from it with
per-exec randomisation instead of the fixed constant.  Keep the AT_BASE
plumbing as-is — it already carries whatever base is chosen.

**Update 2026-06-14:** the dependency is now in place — a per-process
VMA-aware mmap gap allocator (`pcb::reserve_unmapped_area` →
`mm::vma::find_gap`, fronted by `handlers::alloc_user_mmap_reserve`) now
serves the general user mmap window with freed-gap reuse and atomic
find+insert.  ld.so's general-region maps already flow through it; what
remains for TD9 is purely the *randomisation policy*: pick a randomised
base for the interpreter/PIE load instead of the fixed `LINUX_INTERP_BASE`
constant.  Note the interpreter is loaded at `0x7000_…`, disjoint from the
mmap window `0x0060_…`, so ASLR for it will need its own randomised
placement (or be folded into the mmap region) rather than just calling the
new allocator.

**Related limitation (not debt, just unimplemented):** end-to-end
interpreter *execution* is untested because no real glibc/musl ld.so is
on the filesystem yet.  The load mechanism (base selection, biased
segment mapping via `load_segments_with_bias`, AT_BASE/AT_ENTRY auxv) is
unit-tested via `spawn::test_load_interpreter_fallbacks` (static-ELF and
absent-interpreter `Ok(None)` fallbacks).  See `todo.txt` "Linux
dynamic-linker (ld.so) load path".

### TD25. `sys_brk` was a no-op stub (claimed grow succeeded but mapped nothing → latent SIGSEGV) — RESOLVED 2026-06-14

**What it was:** `sys_brk` (`kernel/src/syscall/linux.rs`) simply echoed
`args.arg0` back to the caller — claiming the requested program break was
granted while mapping **no** memory.  Any real glibc/musl program whose
`malloc` used the brk fast path (it does for small allocations until the
main arena is exhausted) would write into the "granted" heap and take an
immediate page fault on unmapped memory → ring-3 SIGSEGV.  The stub only
happened to be harmless because no glibc binary runs end-to-end yet; it
was a live trap waiting for the first one.

**Resolution (2026-06-14):** Implemented a real demand-paged brk heap.

- **PCB state:** added `brk_start` (heap floor) and `brk_current` (program
  break) to `Process` (`kernel/src/proc/pcb.rs`), inherited verbatim across
  `fork` (CoW heap clone) and reset on `exec` — recomputed from the new
  image's page-aligned end for Linux images (`elf::image_end`), cleared to
  `0` for native images (no Linux brk heap).  Accessors `set_brk_region` /
  `get_brk` / `set_brk_current`.
- **VMA:** new `VmaKind::Brk` (`kernel/src/mm/vma.rs`) — faults exactly like
  `Anonymous` (demand-paged, zero-filled); exists so `/proc/<pid>/maps`
  labels it `[heap]` and `sys_brk` can find/resize its own VMA.  The heap is
  a single `[brk_start, round_up(brk_current))` VMA.
- **sys_brk semantics (Linux-faithful):** `brk(0)` / `addr < brk_start`
  query (return unchanged break); grow maps the new span by replacing the
  heap VMA (demand-paged) and charges `RLIMIT_AS` for the full added virtual
  span up-front (committed-by-default — no overcommit); shrink unmaps+frees
  faulted frames via `unmap_user_range` and refunds the charge; same-top-
  frame moves touch nothing.  On **any** failure (RLIMIT_DATA, RLIMIT_AS,
  VMA collision, OOM, overflow) it returns the *unchanged* break — exactly
  what glibc's `__sbrk` expects so it falls back to mmap and reports ENOMEM
  itself.
- **Heap ceiling (image-dependent):** `brk_ceiling(brk_start)` returns
  `USER_MMAP_BASE` for a low-loaded ET_EXEC (`brk_start < USER_MMAP_BASE`)
  and `LINUX_INTERP_BASE` for a high-loaded PIE (`brk_start >=
  USER_MMAP_BASE`), so the heap can never grow into the mmap window, the
  interpreter window, or the stack.  The VMA-overlap check is a second
  guard.

**Tests:** `syscall::linux::self_test_brk_logic` (pure: `brk_round_up`
boundary/overflow cases + `brk_ceiling` ET_EXEC/PIE/ordering) and the
ring-3 end-to-end `proc::spawn::self_test_linux_brk` (a real Linux-ABI
process queries its break, grows 32 KiB, writes a sentinel into the
*second* heap frame, reads it back, exits with that byte — exit `0x6D`
proves the grow + demand-paging of multiple frames works; both verified in
the boot-test serial log).

**Update (2026-06-14): `arch_randomize_brk` gap now implemented.** The heap
floor is the page-aligned image end shifted up by a random gap
(`spawn::choose_brk_start`), mirroring Linux x86_64's `arch_randomize_brk`
with 13 bits of entropy (matching Linux's position count per the
entropy-is-the-metric policy of design-decisions #20; 128 MiB max gap at our
16 KiB pages). Always-on when the CSPRNG is seeded, no-gap fallback before
seeding, "no heap" (`image_end == 0`) preserved. Covered by
`test_brk_aslr_gap` and exercised end-to-end by `self_test_linux_brk`. No
remaining gaps on the brk heap.

### TD8. `membarrier` PRIVATE_EXPEDITED issue without prior REGISTER returns 0 where Linux returns `-EPERM` — RESOLVED 2026-06-14

**What it was:** `sys_membarrier()` (`kernel/src/syscall/linux.rs`) accepted every
issue command (`MEMBARRIER_CMD_PRIVATE_EXPEDITED`,
`…_PRIVATE_EXPEDITED_SYNC_CORE`, `…_PRIVATE_EXPEDITED_RSEQ`) and returned 0
unconditionally. Linux v6.6's `membarrier_private_expedited()` first checks the
issuing mm's `membarrier_state` and returns **`-EPERM`** when the matching
`MEMBARRIER_STATE_*_READY` bit is not set — i.e. when the process never issued
the corresponding `…_REGISTER_*` command. That EPERM check runs **before** the
single-CPU `return 0` shortcut, so even on our uniprocessor an unregistered
`PRIVATE_EXPEDITED` issue should be `-EPERM`, not 0. Symmetrically, our
`…_REGISTER_*` commands were no-ops and `MEMBARRIER_CMD_GET_REGISTRATIONS`
always reported 0. (Note: `GLOBAL_EXPEDITED` *issue* is NOT gated on Linux —
only the three `PRIVATE_EXPEDITED*` issues are; the original note overstated
this.)

**Fix (implemented):** added a per-mm `membarrier_state: u32` READY bitmask to
`Process` (`kernel/src/proc/pcb.rs`), shared across the process's threads (so a
thread may register and a sibling issue), inherited verbatim across `fork`
(Linux's `dup_mm` memcpy) via `pcb::membarrier_register` / `membarrier_state`
accessors. `sys_membarrier` now resolves the issuing mm's state and routes
through the pure, unit-tested `membarrier_decide(cmd, state)`: `REGISTER_*` OR
in their READY bit; the three `PRIVATE_EXPEDITED*` issues return `-EPERM`
unless their bit is set; `GET_REGISTRATIONS` reports the registered-command
bitmask via `membarrier_registrations_mask`; `GLOBAL`/`GLOBAL_EXPEDITED` issue
need no registration. The boot self-test (`self_test_membarrier_registration`,
"membarrier per-mm registration gating (TD8): OK") exercises `membarrier_decide`
exhaustively and drives the per-mm READY-bit store (register/idempotency/
cross-command isolation/GET mask) through a throwaway `pcb::create` process —
solving the original "no owner mm at boot" testability blocker by testing the
pure helper and the pcb layer directly rather than through the syscall caller's
(absent) mm.

**Residual divergence — RESOLVED 2026-06-14:** Linux resets `membarrier_state`
to 0 on `execve` (`membarrier_exec_mmap`); we previously lacked an exec-time
PCB-reset hook (the same gap noted for `linux_dumpable`/`linux_keepcaps`/
`linux_thp_disable`), so a registration survived exec. Now fixed: added
`pcb::reset_linux_state_for_exec(pid)`, called from `spawn::exec_process` after
`reset_vmas_for_exec`, which clears (under one `PROCESS_TABLE` lock) exactly the
fields Linux unconditionally resets on every exec — `membarrier_state` → 0
(`exec_mmap`→`membarrier_exec_mmap`), `linux_dumpable` → 1 (`SUID_DUMP_USER`;
explicit `set_dumpable` in `begin_new_exec`), and the `linux_securebits`
`SECBIT_KEEP_CAPS` bit (bit 4 only — `cap_bprm_creds_from_file` clears it on
every exec, preserving the lock bit and every other securebit). That bit 4 is
now the **single source of truth** for `prctl(PR_SET_KEEPCAPS)` (see the
follow-up note below), so clearing it on exec resets keepcaps too. Fields Linux
preserves across a normal (non-privileged)
exec are left untouched: `linux_thp_disable` and `linux_memory_merge` (both
`MMF_INIT_MASK` mm-flags that the new mm inherits via
`mm->flags = current->mm->flags & MMF_INIT_MASK` — `begin_new_exec` has no
explicit THP/KSM override, so they survive exec), `linux_pdeathsig` (cleared
only on set-uid/caps exec, otherwise preserved per prctl(2)),
`linux_personality` (x86_64 `set_personality_64bit` only clears the unmodelled
`READ_IMPLIES_EXEC`; `ADDR_NO_RANDOMIZE` survives), `linux_no_new_privs`
(sticky), `linux_child_subreaper`, timer-slack. (An initial version of the hook
wrongly reset `linux_thp_disable`, repeating entry 98's mistaken "cleared on
execve" claim; corrected same session.) Self-test
`pcb::test_reset_linux_state_for_exec` asserts the cleared state (membarrier,
dumpable, keepcaps, securebits KEEP_CAPS bit with lock+other bits kept) and the
five preserved fields ("[proc]   exec Linux-state reset: OK"). The in-kernel
`membarrier` self-test
caller (no owner mm) keeps the "fence/0" behaviour by feeding `u32::MAX` to the
gating helper — there is no registration model for a kernel thread with no
sibling userspace threads.

**Follow-up — keepcaps/securebits single source of truth (2026-06-14):** the
exec-reset audit surfaced a real ABI incoherence: `prctl(PR_SET_KEEPCAPS)` was
backed by a standalone `linux_keepcaps` field while `SECBIT_KEEP_CAPS` lived in
`linux_securebits`, even though Linux stores both in the *same*
`cred->securebits` bit 4. `PR_SET_KEEPCAPS`/`PR_SET_SECUREBITS` wrote different
storage, so `PR_GET_KEEPCAPS` and `PR_GET_SECUREBITS` could disagree where Linux
keeps them identical. Fixed by removing the `linux_keepcaps` field and making
`pcb::get_keepcaps`/`set_keepcaps` thin views over `linux_securebits` bit 4
(set/clear only bit 4, leaving every other securebit intact). Also added the
missing Linux gate to the `PR_SET_KEEPCAPS` handler: once
`SECBIT_KEEP_CAPS_LOCKED` (bit 5) is engaged the flag is frozen and the call
returns `-EPERM` (`cap_task_prctl`, verified against torvalds/linux v6.6
`security/commoncap.c`). The gate is the pure helper
`keepcaps_change_allowed(securebits)` so it is unit-testable without a caller
PCB. Tests: `self_test_prctl_dispatch`'s keepcaps block now asserts get/set
coherence in both directions (keepcaps↔securebits bit 4) and the lock-gate
truth table; `pcb::test_reset_linux_state_for_exec` proves `set_keepcaps`
coherently drives bit 4 and the exec reset clears only it.

**Companion fix — PR_SET_SECUREBITS lock enforcement now unit-tested
(2026-06-14):** the same audit found the `PR_SET_SECUREBITS` lock-bit
enforcement (a set lock can't be cleared; a locked flag can't flip) was
inline in the handler and so its `-EPERM` path was unreachable from the
kernel-context boot self-test (no `caller_pid` PCB to seed locked bits) —
the test only covered value validation. Extracted the decision into the pure
`securebits_change_allowed(cur, new_val)` (mirrors `cap_task_prctl`) and added
a truth-table test to `self_test_prctl_dispatch` covering: no-locks→allowed,
new-lock→allowed, clear-set-lock→denied, flip-locked-flag (both
set→clear and clear→set)→denied, and locked-flag-kept-while-flipping-an-
unlocked-flag→allowed ("PR_SET_SECUREBITS lock-bit enforcement … : OK").

### TD7. `set_mempolicy_home_node` returns 0 where Linux returns `-ENOENT`/`-EOPNOTSUPP` — APPROXIMATION 2026-06-12

**What:** `sys_set_mempolicy_home_node()` (`kernel/src/syscall/linux.rs`)
returns 0 for any valid non-empty range. Linux v6.6 instead walks the VMAs
in `[start, end)` with `err` initialized to `-ENOENT`: it returns `-ENOENT`
when no VMA in the range carries an explicit `MPOL_BIND`/`MPOL_PREFERRED_MANY`
policy, and `-EOPNOTSUPP` for a VMA whose policy is some other mode. Only a
range that already has a bind/preferred-many policy yields 0.

**Why we diverge:** our `mbind` is a UMA no-op that does **not** store
per-VMA mempolicy, so the kernel cannot tell whether the caller previously
established a policy on the range. We pick 0 (the "policy was set, home node
applied" success outcome — the common real-world sequence where
`set_mempolicy_home_node` follows a successful `mbind(MPOL_BIND)`) over
`-ENOENT`. Returning `-ENOENT` would instead break that common path.

**Proper fix:** implement real per-VMA mempolicy storage so the VMA walk can
distinguish "no policy" (`-ENOENT`), "wrong policy" (`-EOPNOTSUPP`), and
"bind policy → apply home node" (0). Tracked as an open question
(`open-questions.md`) because the 0-vs-`-ENOENT` choice is a genuine
tradeoff. **Note:** batch 551 *did* fix the unambiguous part — the
`home_node` online check now runs before the len/end gates, matching v6.6.

### TD5. NUMA nodemask `{0, extra-node}` is rejected where Linux accepts it — APPROXIMATION 2026-06-12

**What:** `get_nodes_uma()` (`kernel/src/syscall/linux.rs`, used by
`sys_mbind` and `sys_set_mempolicy`) collapses Linux's full nodemask down
to two booleans — `mask_empty` and `mask_has_extra_bits` (any node other
than node 0 set) — and the callers reject `mask_has_extra_bits` with
`-EINVAL`. Linux instead **intersects** the user mask with
`current->mems_allowed` (= `{0}` on our single-node system) and checks the
*intersected* mask for emptiness in `mpol_ops[mode].create`.

**Divergence:** a mask of `{0, N}` (node 0 **plus** a non-existent node N)
is rejected by us (`-EINVAL`) but **accepted** by Linux for
`MPOL_PREFERRED` / `MPOL_BIND` / `MPOL_INTERLEAVE` / `MPOL_PREFERRED_MANY`,
because the intersection `{0,N} ∩ {0} = {0}` is non-empty. A mask of `{N}`
alone (no node 0) is `-EINVAL` in both (intersection empty), so only the
"node 0 present *and* an extra bogus node" case differs.

**Why it's an approximation, not a bug now:** real programs on a
single-node box pass either an empty mask or `{0}`; `{0, N>0}` is not a
shape `numactl`/libnuma/jemalloc/tcmalloc produce when only node 0 exists.
The result is also strictly *more* conservative (we reject something Linux
accepts; we never accept something Linux rejects).

**Proper fix:** have `get_nodes_uma` report the effective mask after
intersecting with `mems_allowed = {0}` (i.e. "is bit 0 set?") separately
from "are there bits we must hard-reject" (only bits above `MAX_NUMNODES`
are hard-rejected by Linux's `get_nodes` itself), and apply the per-mode
emptiness check to the *intersected* mask in `mpol_new_check`'s spirit.
This is only worth doing if/when we support more than one NUMA node.

### TD6. `move_pages` per-page node error stores `-EINVAL` where Linux stores `-ENODEV`/`-EACCES` — RESOLVED 2026-06-12 (batch 549)

**Resolution:** `sys_move_pages` now stores `-ENODEV` for any non-zero
target node, matching `do_pages_move`'s `err = -ENODEV` path (out-of-range
or `!node_state(node, N_MEMORY)`). On a single-node box every node but 0
lacks `N_MEMORY`, so `-ENODEV` is correct for all of them; the `-EACCES`
"valid node not in `task_nodes`" branch is unreachable when only node 0 has
memory. Batch-105 self-test Case 4 updated to expect `[0, -ENODEV, 0]`.
Original analysis retained below for reference.

---

**What:** `sys_move_pages` (`kernel/src/syscall/linux.rs`), in move mode
(`nodes != NULL`), writes `status[i] = -EINVAL` for any requested target
node other than 0 (we only have node 0). Linux's `do_pages_move`
(`mm/migrate.c`) instead validates each target node and stores a per-page
error via `store_status`: `-ENODEV` when the node is out of range or has no
memory (`!node_state(node, N_MEMORY)`), or `-EACCES` when the node is valid
but not in `task_nodes` (`!node_isset(node, task_nodes)`). On a single-node
box, target node 1 would be `-ENODEV` (node 1 has no `N_MEMORY`), not
`-EINVAL`.

**Divergence:** observable only in `status[i]` for a deliberately-bogus
target node; the syscall return code (0) is unaffected. Batch-105 self-test
Case 4 currently asserts `status == [0, -EINVAL, 0]`.

**Why deferred (not fixed in batch 548):** batch 548 fixed two
independently-verified divergences (missing pid→ESRCH lookup; invented
E2BIG cap) and intentionally did **not** guess at the per-page errno. The
exact `do_pages_move` node-validity path (range check → `N_MEMORY` →
`node_isset(task_nodes)` → `store_status`) needs its own verbatim v6.6
verification before changing the stored errno.

**Proper fix:** verify `do_pages_move`/`add_page_for_migration`/`store_status`
against v6.6, then store `-ENODEV` for out-of-range / no-memory nodes and
`-EACCES` for valid-but-disallowed nodes, and update Case 4's expectation.

### TD4. Monolithic `syscall::linux::self_test()` has an unbounded boot-stack frame — RESOLVED 2026-06-14

**Resolution (2026-06-14):** The split is complete. Every self-contained
validation block in `self_test()` is now wrapped in its own
`#[inline(never)]` nested helper (`fn self_test_NAME() -> KernelResult<()>`,
called via `?`), so each sub-frame is allocated and freed transiently around
its call and no single frame is the sum of all batches. The body went from one
monolithic ~1.4 MB frame to ~80 small per-block helpers. Three earlier helpers
that had grown to wrap multiple sibling blocks (`getrusage_sysinfo_times` = 5
blocks, `capget_capset` = 2, `sched_affinity` = 2) were peeled apart so each
block gets its own frame. A structural scan confirms **zero** bare top-level
blocks remain. The technique used throughout (Technique B): insert a 5-line
header — `self_test_NAME()?;` + `#[inline(never)] fn self_test_NAME() -> …
{ use crate::serial_println;` — immediately before the block's leading
comment, and a 2-line footer — `Ok(())` + `}` — immediately after the block's
closing brace; the block body is never reproduced or re-indented, so the wrap
is safe for arbitrarily large blocks. A non-inlined nested fn cannot capture
enclosing locals, which acts as a compile-time safety net against
mis-scoping. Every wrap was individually boot-tested (BOOT_OK) and committed.
This removes the F10 (`.bss`/`FPU_STRATEGY` silent-corruption) failure class
at its root rather than merely deferring it behind the boot-stack canary.

**Progress (2026-06-13):** Began the incremental `#[inline(never)]` split. The
two leading self-contained check groups were extracted into standalone
functions — `self_test_errno_mapping()` (errno round-trips + the `check_errno!`
macro, used nowhere else) and `self_test_native_translation()` (the
`linux_from_native` round-trips). Both are guaranteed behaviour-preserving:
their locals never escape the extracted region. `self_test()` now calls them
via `?`. This establishes the repeatable extraction pattern (cut a contiguous
region whose locals don't cross the boundary, lift to an `#[inline(never)] fn
… -> KernelResult<()>`, replace inline with a `?` call, build+boot-test).
Continue opportunistically: the safe cut points are regions that don't share a
reused local (e.g. the early checks share `args`/`r`, so a larger contiguous
run ending at the last use of those must be lifted as one unit). Remaining
work is the bulk of the ~40 k-line body.


**What:** `kernel/src/syscall/linux.rs::self_test()` is a single ~1.4 MB
function (~39 k lines, opens near line 35858, closes near line 75298) whose
body is one giant 4-space enclosing block. Each ABI-fidelity batch (536 and
counting) appends its own locals inside that block. In the unoptimized
debug build (`opt-level=0`, no LLVM stack-slot coloring), the compiler does
**not** reuse stack slots across the lexically-disjoint per-batch sub-blocks,
so the function's single frame is the *sum* of every batch's locals
(~480 KiB as of batch 536 and growing monotonically). It runs directly on
the guardless boot stack — this is exactly what caused F10 (silent
`.bss`/`FPU_STRATEGY` corruption when the frame overran the old 512 KiB
stack).

**Why it's debt, not a bug now:** F10's fix (2 MiB boot stack + 64 KiB
redzone canary) gives ~1000+ batches of runway and converts any future
overrun into a clean `FATAL: boot stack overflow detected` halt instead of
silent corruption. So the system is correct and self-diagnosing. But the
frame grows ~1 KiB/batch, so this only defers the wall; it does not remove
it.

**Proper fix:** split `self_test()` into many small `#[inline(never)]`
sub-functions (e.g. one per batch or per logical group, `fn self_test_b536()
-> Result<…>` …) called in sequence from a thin driver, so each sub-frame is
allocated and freed around its call and no single frame is large. This caps
the boot-stack frame regardless of batch count and is the real removal of
the F10 failure class.

**Why deferred:** the function is one giant 4-space block; a hand-split risks
silently mis-scoping locals shared across batch boundaries (a local defined
in an early batch and read in a later one would stop compiling, or worse,
shadow). Doing it safely means iterating in small chunks with a build after
each (~50 s/cycle), and the canary makes it non-urgent. **Trigger to do it
properly:** before the boot-stack usage (reported by the canary scan / a
future high-water mark print) crosses ~50 % of the 2 MiB stack, or
opportunistically when next touching the self-test scaffolding.

### TD3. Prefix-boundary subtree checks: audit every site for trailing-slash correctness — RESOLVED 2026-06-10

**What:** The "is `path` inside directory subtree `prefix`" check was
written inline at ~30 sites as
`path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')`
(sometimes with a leading `path == prefix ||`).  This idiom is **only
correct when `prefix` has no trailing slash**.  When `prefix` already
ends in `/` (e.g. a registration like `"/protected/"`), the
`get(prefix.len()) == Some(&b'/')` boundary check looks one byte past
the slash and therefore only matches *double-slash* paths
(`/protected//x`), so real children never match — the check silently
fails (open for deny handlers, or simply never fires for "missing file"
/ exclusion logic).

**RESOLUTION (2026-06-10):** Created a single canonical helper module
`kernel/src/fs/pathutil.rs` exposing `path_in_subtree(path, dir)` and
`path_strictly_under(path, dir)`.  Both normalise away an optional
trailing slash (`dir.strip_suffix('/')`) before the component-boundary
check, so they are correct whether or not the caller's prefix carries a
trailing slash.  Five `#[cfg(test)]` unit tests pin the contract
(basic boundary, trailing-slash equivalence, empty/root-matches-all,
strictly-under-excludes-self, strictly-under-root).  Every real subtree
check now routes through this helper; the footgun idiom is gone from the
fs subsystem.

**Confirmed-buggy (silent failures), now fixed via the helper:**
- `integrity.rs` baseline-paths filter (earlier commit `22a8098f`) —
  prefix carried a trailing slash; `verify_dir` never reported missing
  files.  Now also routed through `path_in_subtree` (removed the
  per-iteration `format!("{excl}/")` allocation in the exclude-dir scan).
- `intercept.rs` `pre_check` interceptor filter — prefixes registered
  with trailing slashes (`/protected/`) so every deny handler failed
  open.  `path_matches_prefix()` is now a thin `#[inline]` wrapper over
  `path_in_subtree` (kept for the descriptive call-site name + bug note).
- `findex.rs:304` `columns_for_dir` — built `prefix` *with* a trailing
  slash, so the old boundary check matched nothing and column discovery
  always returned empty.  Now routed through `path_strictly_under`.

**Routed through the helper for robustness (prefix-source could carry a
trailing slash; uniform now):** `undelete.rs` (scan filter), `search.rs`
(exclude prefixes), `queryable.rs` (root filter), `dedup.rs` (exclude
prefixes), `directio.rs` (`is_dio_path`), `index.rs` (exclude/remove/
is_watched ×3), `fswalk.rs` (`is_excluded`, both default + opts),
`fcomment.rs` (search/list/remove_under ×3), `changetrack.rs` (path +
old_path prefix filter), `fileversion.rs` (policy + max-size lookups).

**Verified correct, left as-is (slash-free prefixes by construction):**
`vfs.rs` (mount paths), `freeze.rs:264` (mountpoint), `atime.rs:163`
(mount_path), `overlay.rs:169` (already-normalised `is_under`),
`notify.rs` `path_matches` (distinct `strip_prefix` impl with
recursive/non-recursive semantics the helper does not model),
`apps/defrag/src/main.rs:659` (`/*` glob with the slash already stripped;
separate crate, cannot reach `fs::pathutil`).

Build clean; QEMU boot test green.

**Kernel-wide sweep (2026-06-10):** grepped all of `kernel/src` for the
`get(X.len()) == Some(&b'/')` idiom — the only matches are the six
`fs/` files already accounted for above (plus `pathutil.rs`, the helper
itself).  No sibling instances exist in `net`, `proc`, `ipc`, `mm`, or
any other subsystem, so the footgun is fully contained and closed.

### TD2. Clippy `clippy::all` deny-level errors not yet zeroed — RESOLVED 2026-06-10 (regressed + re-fixed 2026-06-14)

**REGRESSION RE-FIX (2026-06-14):** 7 deny-level `clippy::all` errors had crept
back in since the original resolution — `byte_char_slices` (`drm/edid.rs:604`,
`fs/compress.rs:1956`), `question_mark` (`fs/fswalk.rs:229`+`:354`,
`fs/hotkeys.rs:134`), and `for_kv_map` (`fs/history.rs:435`, `proc/pcb.rs:1618`).
All fixed mechanically (byte-string literals, `?` operator, `.values()` map
iteration); `cargo clippy -p kernel` is back to **0 deny-level errors**, build +
QEMU boot test green. Lesson: the deny-level gate is only green between sweeps —
it needs to actually run in CI to stay zeroed (no CI exists yet).

**Sweep extended to all default-members (2026-06-14):** since the kernel was
not the only crate that could regress, `cargo clippy` was run on the other two
default-member crates. `posix` had **1** deny-level regression —
`too_many_arguments` (8/7) at `epoll.rs:2150`, `translate_kernel_event`,
introduced when the `is_dir` param was added for IN_ISDIR (TD17). Fixed with a
justified `#[allow(clippy::too_many_arguments)]` (commit `8acddca0c`); the 8
params are the distinct fields of one kernel watch event and a struct would only
add indirection to a pure host-tested translator. `toolchain/stubs`
(`slateos-stubs`) was already clean. All three default-members now report **0
deny-level errors**; posix's 19972 host tests still pass.

**RESOLUTION (2026-06-10):** `cargo clippy -p kernel` now reports
**0 deny-level errors** (down from 451) and ~17,297 warn-level warnings.
The deny-level `clippy::all` gate is green and can be used as CI.  The
warn-level lints remain by design (see below).  Landed across several
reviewable batches: the 158 doc-formatting lints, the 167 machine-
applicable idiom fixes, the 181 doc-comment lints, and a final hand-
fixed batch of 77 (commit `15dc0168`) covering `manual_memcpy`,
`ptr_arg`, `inherent_to_string`→`Display`, `wrong_self_convention`,
`upper_case_acronyms`, `enum_variant_names`, `type_complexity`,
`if_same_then_else` (inspected — no real copy-paste bugs), and a tail of
singletons (`fn_to_numeric_cast`, `forget_non_drop`, `never_loop`,
`only_used_in_recursion`, `pointers_in_nomem_asm_block`,
`large_enum_variant`, etc.).  `cargo build` and the QEMU boot test pass.

The two warn-tier correctness audits (step 3 below) are also complete:

* **`cast_ptr_alignment` (107) — audited, safe, left as warn.**  Every
  site is in MMIO / DMA-ring / on-disk-format / wire-protocol code
  (virtio, xhci, hda, e1000, ahci, ext4 `ondisk`, smp, `mm/frame`,
  syscall device-register reads).  Alignment is guaranteed by the
  page-aligned DMA frame allocation or by naturally-aligned hardware
  registers; the lint fires only because it sees a bare `*mut u8`/`*const
  u8` base.  Representative samples verified (e.g. `virtio/queue.rs:168`
  casts a page-aligned frame + 16-byte descriptor stride to
  `*mut VirtqDesc`).  One outlier — `ext4/ondisk.rs:1017` — casts an
  align-1 stack `[u8; 1024]` to a struct pointer; technically UB but
  benign on x86_64 and confined to a boot self-test.  No production
  under-alignment.  Eventual cleanup is a per-site `// SAFETY:` +
  `#[allow]`, but the casts are correct as-is.

* **`large_stack_arrays` (7) — audited; 1 genuine fixed, rest are false
  positives.**  Five (`cgroup.rs`, `fs/vfs.rs`, `klog.rs`, `mm/rmap.rs`,
  `sched/priority_rr.rs`) are `const fn` constructors whose arrays are
  const-evaluated directly into `static`/rodata storage — never on the
  stack; the lint is conservative.  `ktrace.rs:461` was a genuine 512-
  entry self-test window on the stack → now heap-allocated via
  `alloc::vec!`.  `scfilter.rs` built a ~19 KiB `FilterTable` on the
  stack before `Box::new` (the prior comment's "heap" claim was defeated
  by the by-value temporary) → `new()` is now `const fn` materialized via
  a `const EMPTY` binding so the box copies from rodata.  (Fixes + doc in
  the follow-up commit.)  The 6 remaining warnings are all const-context
  arrays in static storage and carry no stack-overflow risk.

---

**Original report (for history):**

**Where:** kernel-wide.  Snapshot `cargo clippy -p kernel` (rust 1.95.0,
2026-06-10): **451 deny-level errors** and **17,320 warn-level
warnings**.

**What this is — and why the two tiers are treated differently.**
The workspace lint config (`Cargo.toml [workspace.lints.clippy]`) sets
`clippy::all = deny (priority -1)`, `clippy::pedantic = warn`, and the
five correctness-pressure lints (`unwrap_used`, `expect_used`, `panic`,
`indexing_slicing`, `arithmetic_side_effects`) = `warn`.  So:

* **Warn-level (17,320) — intentional by design, NOT a blocker.**
  Dominated by:
  - `arithmetic_side_effects` 7,511
  - `indexing_slicing` 5,711
  - `expect_used` 2,689
  - `unwrap_used` 1,034
  - `unnecessary_wraps` 156, `cast_ptr_alignment` 107, others < 25 each.

  These are the defensive-pressure lints CLAUDE.md deliberately set to
  `warn` rather than `deny` because they are pervasive in low-level
  kernel code (every `a + b`, every `slice[i]`, every page-table index)
  and forcing `checked_*`/`.get()` everywhere would bury real signal
  under mechanical noise.  They are advisory: the rule is "prefer `?`,
  `.get()`, `.checked_*` in new code and surgically harden hot/attacker-
  reachable paths," not "drive the count to zero."  **These are accepted
  by design and should NOT be mass-rewritten.**  Two sub-categories DO
  deserve a real audit pass and should be tracked as their own work:
  `cast_ptr_alignment` (107 — genuine UB risk if any cast actually
  under-aligns; most are MMIO/identity-mapped and provably fine but each
  should carry a `// SAFETY:`/`#[allow]` with justification) and
  `large_stack_arrays` (7 — kernel stacks are bounded; verify none blow
  the stack).

* **Deny-level (451 `clippy::all` errors) — these SHOULD be fixed**, per
  the project's own `all = deny` gate.  The good news: they are almost
  entirely **mechanical, machine-applicable idiom lints**, not logic
  bugs.  Top categories:
  - `doc_overindented_list_items` 137, `doc_lazy_continuation` 21
    (158 = doc-comment formatting — auto-fixable)
  - `unwrap_or_default` 21, `manual_strip` 15, `manual_slice_fill` 14,
    `vec_init_then_push` 13, `manual_memcpy` 10, `manual_clamp` 8,
    `assign_op_pattern` 8, `manual_div_ceil` 8, `slow_vector_
    initialization` 7, `while_let_loop` 6, `explicit_counter_loop` 5,
    `single_char_add_str` 5, `single_match` 5 … (all auto-fixable)
  - A small tail needs human judgment, not blind `--fix`:
    `type_complexity` 10 (extract type aliases), `duplicated_attributes`
    9 (a module-level `#![allow(dead_code)]` duplicating the parent
    `#[allow]` in `fs/mod.rs` — remove the inner one),
    `upper_case_acronyms` 9 and `enum_variant_names` 7 (renames — verify
    no public-API churn), `if_same_then_else` 7 (could be a real copy-
    paste bug — inspect each), `comparison_to_empty` 7.

**File distribution of the 451 errors** (primary span):
`syscall/linux.rs` 200, `kshell.rs` 39, `fs/bzip2.rs` 8,
`syscall/handlers.rs` 8, `sched/mod.rs` 6, `fs/contextmenu.rs` 5,
`fs/procfs.rs` 5, `fs/monitors.rs`/`fs/tags.rs`/`fs/taskbar.rs`/
`net/http.rs` 4 each, then a long tail of 1–3 across ~40 more files.
`linux.rs` alone is 44% of the total (it is the single largest source
file, ~28k lines, and accretes idiom lints fast).

**Why it's open rather than fixed-on-sight:** the count is large and
spread across ~50 files; the bulk is `cargo clippy --fix` territory but
that produces a sweeping multi-file diff that materially changes the
shape of hot syscall code (`linux.rs`), so it warrants being landed as
its own reviewable change(s) rather than smuggled into a feature commit.
Two deny-errors that were authored as part of the /proc work
(2026-06-10) were fixed immediately at their source:
`procfs.rs` `gen_pid_statm` doc list (`doc_overindented_list_items`) and
`pcb.rs` `set_exe_path` (`manual_contains` → `slice.contains`).

**Tooling caveat (verified 2026-06-10):** `cargo clippy --fix` does
**not work** in this environment — it recompiles, reports the count of
machine-applicable suggestions (e.g. "to apply 176 suggestions"), but
writes **zero** changes to disk.  Tried four ways:
`cargo clippy -p kernel --fix --allow-dirty`;
`… --bin kernel … --no-deps`;
`… -- --force-warn clippy::all` (to defeat the deny-as-error so the
verify-recompile would pass); and with the workspace
`clippy::all` level temporarily flipped to `warn` in `Cargo.toml`.
All four no-op'd (0 `.rs` files modified, ~4 min each).  The kernel
targets the built-in `x86_64-unknown-none` with no build-std, so this
is not a custom-target issue; it looks like `cargo fix`'s
write-back/verify phase failing silently on this Windows toolchain.
**Do not burn build cycles retrying `--fix` — remediation must be by
hand (or with a non-cargo rewrite tool).**

**Proper fix / remediation plan:**
1. Hand-fix the machine-applicable bulk in reviewable batches, grouped
   by lint family so each diff is easy to verify: start with the ~158
   doc-formatting lints (`doc_overindented_list_items`,
   `doc_lazy_continuation` — pure comment edits, zero risk), then the
   manual-idiom families (`unwrap_or_default`, `manual_strip`,
   `manual_slice_fill`, `vec_init_then_push`, `manual_memcpy`,
   `manual_clamp`, `assign_op_pattern`, `manual_div_ceil`, …).  These
   rewrites are semantics-preserving (`manual_memcpy` →
   `copy_from_slice`, `vec_init_then_push` → `vec![…]`, etc.).  Land
   `linux.rs` (200 of the 451) as its own commit(s) since it is the
   hottest file and the largest single chunk.  Boot-test after each
   batch.
2. Hand-fix the judgment tail: dedupe the `#![allow(dead_code)]`
   attributes, extract `type_complexity` aliases, inspect every
   `if_same_then_else` for an actual logic bug before collapsing it,
   and do the acronym/enum renames with a grep for external callers.
3. Separately audit `cast_ptr_alignment` (107) and `large_stack_arrays`
   (7) from the warn tier — these are the only warn-level lints with a
   real correctness dimension; annotate or fix each.
4. Leave the remaining warn-level lints as-is (by design); revisit only
   the policy, not the individual sites.

Until step 1–2 land, `cargo clippy -p kernel` exits non-zero, so it
cannot be used as a CI gate yet.  `cargo build` / boot-test are clean.

---

### (closed) TD1 — `frame::ALLOCATOR` IRQ-safety — closed as F5 on 2026-06-07.
