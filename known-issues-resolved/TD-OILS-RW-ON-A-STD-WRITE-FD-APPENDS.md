### TD-OILS-RW-ON-A-STD-WRITE-FD-APPENDS. `1<> file` / `2<> file` write at the end instead of at offset 0, and the read half does not see it — 2026-08-01 — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — the `RedirectOp::ReadWrite` arm of
`Shell::resolve_one_redirect`, the `fd == 2` / `else` branches, which record
`plan.stderr = Some((path, true))` / `plan.stdout = Some((path, true))`: a
*path* to be reopened in append mode, rather than the `wr` handle
`open_rw_pair` just cloned from the read half.

**Reproduce** (`target/dvscratch/t3/q3.sh`):

```sh
printf 'one\ntwo\n' > in
( { echo W; } 1<>in )            # bash: in = "W\ne\ntwo\n"   osh: "one\ntwo\nW\n"
( { read -r l <&1; } 1<>in; echo "l=[$l]" )   # bash: l=[W]   osh: l=[one]
```

TD-OILS-RW-OFFSET fixed this for fd 0 (`plan.stdin` + `plan.stdin_write`, one
handle `try_clone`d) and for fd ≥ 3 (`ExtraFdOp::ReadWrite(rd, wr)`), but fd 1
and fd 2 kept the older path-and-append representation because `RedirPlan` has
a *path* slot for them and no handle slot. Two consequences: the write lands at
EOF rather than at offset 0 (so `1<> file` behaves as `>> file`), and — since
2026-08-01, when `stdout_read` / `stderr_read` gave fd 1 and fd 2 a read half
at all — the read half is a different open file description from the write
half, so neither sees the other's position.

**Fixed** as proposed: `RedirPlan` gained `stdout_write` / `stderr_write:
Option<Arc<File>>` beside the existing path slots, exactly as `stdin_write`
sits beside `stdin`, set by the `RedirectOp::ReadWrite` arm from the `wr` half
`open_rw_pair` already produced. Nothing new is opened; the handle is
*duplicated* where it is set, which is the whole point — a reopen is a second
open file description with an offset of its own.

One helper, `open_std_sink(cwd, handle, path, append)`, expresses "the handle
wins where it is set" once, and every place that turned a plan's fd-1 / fd-2
sink into a `File` now goes through it:

| site | shape it serves |
|---|---|
| `Shell::exec_with_redirects` (stdout, and stderr's non-shared branch) | `{ …; } 1<>f`, every compound body |
| `Shell::run_external`'s `redir.stdout` / `redir.stderr` arms | `sh -c 'echo W' 1<>f` |
| `Shell::run_builtin_body` → `BuiltinStdout::rw` → `Shell::write_redirected` | `echo W 1<>f`, a builtin writing in pieces |
| `Shell::push_builtin_stderr`, `Shell::push_partial_stderr` | `cd /nosuchdir 2<>f` |
| the `exec` builtin's plan path | `command exec 1<>f` |

The path slot is kept beside the handle rather than replaced, because it is
what the rest of the plan is asked about: whether fd 1 is writable at all
(`stdout_is_read_only`), and whether a same-path `2>&1` beside it is a dup
(`stderr_shares_stdout`). The two `2>&1` / `1>&2` branches of
`Shell::resolve_dup_out` copy the handle and the read half across with the
path, so `1<>f 2>&1` gives fd 2 the same description rather than a second open.

`exec 1<> file` already worked: `apply_persistent_redirect` has its own
`ReadWrite` arm that installs both halves as handles, and only the *scoped*
path was wrong.

**Test.** `tests/corpus/a-read-write-source-on-a-std-fd-keeps-one-offset.sh`
(33 shapes: offset-0 writes, no truncation, creation, the shared cursor in both
orders, dups, list ordering, external children, `exec`, and every body that
carries its own redirect list) and the unit test
`read_write_on_a_std_write_fd_writes_at_the_shared_offset`.

**One deliberate divergence** came out of the measurement, recorded as
TD-OILS-CMDSUB-RW-ON-STDOUT-DROPPED below.
