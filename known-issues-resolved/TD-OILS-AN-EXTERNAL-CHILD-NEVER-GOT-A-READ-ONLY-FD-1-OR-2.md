### TD-OILS-AN-EXTERNAL-CHILD-NEVER-GOT-A-READ-ONLY-FD-1-OR-2. `sh -c 'echo W' 1<in` printed `W` on the terminal — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::run_external`'s fd 1 and fd 2
wiring, and `child_read_only_out`.

**What.** `1< file` binds fd 1 to a description with no write half; the shell's
own builtins had answered a write through it with `EBADF` for as long as
`tests/corpus/a-std-fd-bound-to-a-read-only-source.sh` has existed. An external
command reached neither branch: `redir.stdout_read` was only ever consulted for
the `>&N` and `exec` routes, so a *direct* `1< file` / `1<<HD` on a spawned
command fell through to `Stdio::inherit()` and the child wrote to the shell's own
terminal.

```text
$ sh -c 'echo W' 1<in; echo "rc=$?"
bash: sh: line 1: echo: write error: Bad file descriptor / rc=1
osh : W / rc=0                       # the child inherited the terminal

$ sh -c 'echo W >&2' 2<in; echo "rc=$?"
bash: rc=1        # nowhere to report it, so only the status survives
osh : W / rc=0
```

Note this is *not* TD-OILS-EXTERNAL-CHILD-HAS-NO-FD-3, which is about fd 3 and
above and stays open: fd 1 and fd 2 are passed to the child, and it was only the
read-only shape of them that was never wired.

**Fix (2026-08-06).** `run_external` consults `RedirPlan::stdout_is_read_only`
/ `stderr_is_read_only` before every other fd 1 / fd 2 route and hands the child
the descriptor via `child_read_only_out`. A byte snapshot — a here-document or
here-string — has no OS object, so `child_read_only_out` now makes one: a pipe
whose *read* end goes to the child (a write down it is `EBADF`, as for a
read-only file, and a child that reads it instead gets the document's text).
It previously handed `Stdio::null()`, which accepts writes — the exact bug the
function exists to prevent.

**Residue, deliberately left.** bash spills a here-document to a temp file and
the child shares its *offset*, so a child that reads the descriptor leaves the
shell less to read. osh copies the remaining bytes into the pipe rather than
consuming them, so `exec 3<<HD; sh -c 'cat <&1' >&3; read -r l <&3` leaves `l`
empty in bash and set in osh. Consuming instead would fix that one script and
break the likelier one (`exec 3<<HD; cmd >&3; read -r l <&3`, where bash's child
never read and the shell still finds the text). The faithful fix is to
materialise the snapshot as a real read-only temp file the first time a child
needs the descriptor, converting the `InputSrc::Bytes` to an `InputSrc::File` at
the same offset — which is what bash does from the start.

**Tests.** `tests/corpus/a-std-fd-bound-to-a-read-only-source.sh` gained an
external section — `1<in`, `2<in`, a here-string, a dup, a persistent `exec`, a
child that reads and then fails to pass it on, and a `1<in >out` control. The
external shapes were previously named in that case's header as deliberately
absent. The case fails without the fix.
