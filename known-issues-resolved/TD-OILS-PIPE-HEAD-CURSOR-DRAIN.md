### TD-OILS-PIPE-HEAD-CURSOR-DRAIN. A pipeline's head stage drains a `< file` cursor instead of sharing its offset — ✅ RESOLVED 2026-07-28

**Where:** `userspace/oils/src/interp.rs` — `pipeline_head_stdin`, the
`StdinSrc::Cursor` arm (cited from its doc comment).

**What.** When a pipeline's head stage inherits a `Cursor`-backed fd 0 (a
compound `< file`, a here-doc), osh reads the cursor **to the end** and pushes
those bytes down a fresh OS pipe. bash's stage shares the shell's file offset
instead, so it consumes only what it reads:

```sh
{ head -n 1 | cat; head -n 1; } < four.txt   # bash: r1 r2   osh: r1
{ read a | cat; read b; echo "[$b]"; } < four.txt  # bash: [r2]  osh: []
```

**Why it is this way.** The drain predates `StdinSrc` being shareable. Now that
`share()` exists the stage *could* be handed the cursor itself — but a stage
holding the cursor is a stage that no longer streams: the bytes would be read
inline instead of by the short-lived writer thread, and an unbounded upstream
would have to be buffered. That is why the change was not folded into
TD-OILS-BG-STDIN-REDIR.

**Proper fix.** Same root cause as that entry's Residual 2: model `< file` as a
real shared `File` handle (one OS description, one offset) rather than a byte
snapshot, and hand stages and jobs a `try_clone` of it. Then neither the head
stage nor an external child has to choose between streaming and sharing.

**Impact.** Narrow: a pipeline head that reads *part* of a redirected input and
leaves the rest for a later command in the same scope.

**Fix (2026-07-28).** The proper fix above, in full: **an input descriptor is now
a real shared open file, not a byte snapshot.**

1. **`InputSrc`** replaces the bare `io::Cursor<Vec<u8>>` behind fd 0 and
   `open_fds`: `enum InputSrc { Bytes(io::Cursor<Vec<u8>>), File(FileInput) }`,
   shared as `InputFd = Arc<Mutex<InputSrc>>` (the old `ByteCursor`). `< file`,
   `<> file`, `exec < file` and `exec N< file` all build the `File` variant from
   one real `File::open` performed when the redirect is applied; only sources
   with no OS object behind them — a here-document, a here-string, a process
   substitution's output — stay `Bytes`. `StdinSrc::Cursor` is accordingly
   `StdinSrc::Fd`, and `snapshot_cursor` is **deleted**: every duplication of an
   input descriptor now shares, because every one of them corresponds to a
   `fork`/`dup2` in bash — `M<&N` (`clone_input_fd`), `clone_for_subshell`'s
   `exec_stdin` and `open_fds`, and `plan.stdin` → `exec_with_redirects`.
2. **`FileInput` mirrors bash's `sync_buffered_stream`.** Read-ahead is what
   makes a shared offset hard: a buffered reader that has consumed 4 KiB to
   return one line leaves the OS position 4 KiB too far for whoever reads next.
   `FileInput::sync` seeks back over the unconsumed remainder, and is called
   both by `share()` (before handing out a `try_clone`) and by `Drop` (so a
   short-lived stage or subshell handle returns its read-ahead). A non-seekable
   source gets a 1-byte buffer, so there is never a remainder to unwind.
3. **Children and stages get the descriptor, not the bytes.** `child_input`
   returns `ChildIn::Handle(File)` for a file-backed source — `run_external`
   passes it straight to `Stdio::from`, and `pipeline_head_stdin` returns
   `HeadIn::File` — falling back to `ChildIn::Bytes` (the old writer-thread
   pipe) only for a snapshot source or a failed `try_clone`. So neither the head
   stage nor an external child has to choose between streaming and sharing.
4. **Command substitution shares too.** A `$( … )` never sees the `&StdinSrc`
   parameter — only `Shell::exec_stdin` — so `exec_with_redirects` now
   scope-overrides `exec_stdin` as well, symmetric with its existing
   `exec_stdout`/`exec_stderr` overrides. That is what makes
   `{ x=$(read a; echo "$a"); read b; } < f` leave `b=r2`.

Covered by `tests/corpus/redirect-shared-input-offset.sh` (new; byte-identical
to bash 5.2 across builtins, externals, subshells, command substitutions,
pipeline stages, `exec <`, `exec 3<`, `exec 4<&3`, here-documents, `&` jobs and
`while read` loops) and five `interp.rs` unit tests
(`shared_input_offset_*`, `simple_command_input_redirects_are_separate_opens`).
