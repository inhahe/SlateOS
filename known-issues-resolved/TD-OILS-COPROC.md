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
  - *Read end* (`NAME[0]`): **this half is now largely done for free.** The
    plan above was written when the read path was Cursor/byte-clone based
    throughout; since 2026-07-28 (TD-OILS-PIPE-HEAD-CURSOR-DRAIN) `open_fds`
    holds `InputFd = Arc<Mutex<InputSrc>>` where `InputSrc::File` is a **live
    `File`** with faithful read-ahead accounting, shared — not snapshotted —
    by `clone_input_fd` (`M<&N`), `clone_for_subshell`, and every child or
    pipeline stage (via `child_input` → `try_clone`). So the dedicated
    `coproc_read_fds` table and the extra `StdinSrc::Live` variant are **no
    longer needed**: convert the parent-side `PipeReader` → `File` (via
    `OwnedHandle`/`OwnedFd`, cfg-split, same as the write end) and install it
    as `file_input(f)` in the existing `open_fds`. `read -u N`, `M<&N`, the
    transient `<&N` `ExtraFdOp::Input` path and `run_external`'s fd-0
    inheritance then all work unchanged, with the correct shared-handle
    semantics a subshell inheriting a coproc fd requires.
  - Two caveats for the live-pipe case specifically: `FileInput::new` probes
    `stream_position()` and gives a non-seekable source a 1-byte buffer, so a
    pipe is read unbuffered and never over-reads — correct, but check the
    cost if a coproc is read in bulk (a `BufRead`-shaped consumer such as
    `read` is fine; consider a larger buffer plus "never read past a newline"
    only if profiling shows it matters). And `FileInput::sync`'s seek-back is
    a no-op on a pipe precisely because nothing is ever buffered ahead.
  - Remaining surface is therefore just the executor + the write-end
    conversion; the 15 buffer-only `open_fds` sites stay untouched.
- **`NAME_PID`:** the body runs as a *thread*, not an OS process, so there is
  no real child pid — assign a synthetic monotonic pid (same limitation osh
  already has for backgrounded shell bodies via `last_bg_pid`). `wait`/`kill`
  on a coproc pid is therefore best-effort.
- **Lifecycle:** join the coproc thread and drop both parent-side `File`
  endpoints on shell exit, on `unset NAME`, and (bash) when a second coproc
  replaces the first. Closing `parent_stdin_w` (via `exec {NAME[1]}>&-` or
  drop) delivers EOF to the coproc's stdin; the coproc's thread finishing
  drops `child_stdout_w`, delivering EOF to the parent's `NAME[0]` reader.
