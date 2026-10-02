### TD-OILS-A-READ-ONLY-FD-2-IS-HANDED-TO-AN-EXTERNAL-CHILD-AS-AN-EMPTY-STREAM. `cmd 2>&3` with a read-only fd 3 lets the child's write succeed — 2026-08-06 — ✅ FIXED 2026-08-06

**Fix (2026-08-06).** `WriteFd::ReadOnly` now carries the description it names,
as `ReadOnlySrc = Option<InputFd>` — the same `Arc` the read side holds, so
nothing is duplicated until a child actually asks for it. `StderrTarget::Discard`
carries the same payload for the same reason. A new `child_read_only_out()`
turns that payload into a child `Stdio` by `share()`ing the underlying file,
falling back to `Stdio::null()` only where there is genuinely no OS object to
hand over (a here-document's byte snapshot, a directory, an already-closed
source, or a handle the host declines to duplicate). Every child-wiring site now
routes through it: the transient `cmd 1>&N` / `cmd 2>&N` arms, the
`exec_stdout_shadowing` arm, the persistent `exec_stdout` / `exec_stderr` arms,
`child_stdio_for_stderr`'s two, and `child_out_from_write_fd` — which no longer
answers `ReadOnly` with an `Err`, since bash's dup of a read-only descriptor
*succeeds* and it is the child's own write that fails.

The doubt that had kept this open was portability: `WriteFd::ReadOnly` models an
access mode precisely *because* Windows answers the shell's own write to a
read-only handle with `ERROR_ACCESS_DENIED`, which would print "Permission
denied" where bash prints "Bad file descriptor". That concern turns out not to
reach the child. Measured with a Python probe: a native child handed a read-only
handle as its stdout answers its write with **`Errno 9` / `EBADF`** — the same
errno a Unix child gets — so handing the descriptor over reproduces bash exactly,
message as well as status. The access-mode modelling remains right for the
shell's own writes and wrong for nothing else.

This also closes **residual divergence 1 of TD-OILS-FD0-WRITE**: an external
child's `>&0` on a read-only fd 0 now fails inside the child with its own
`echo: write error: Bad file descriptor`, where the shell used to refuse the
redirect with `osh: 0: Bad file descriptor` (same status, different wording).

**Tests.** `tests/corpus/redirect-fd0-write.sh` gained three sections — fd 2 via
`exec 3< f` and via `2>&0`, in the transient / `exec` / group forms; the fd 1
form where the child's own diagnostic is visible; and `1>&2` onto a read-only
fd 2 — all byte-identical to bash 5.2.37.

**Original report follows.**

**Where:** `userspace/oils/src/interp.rs` — the external-command stderr wiring:
the `Some(WriteFd::ReadOnly)` arm under `exec_stderr` and the
`Some(StderrTarget::Discard)` arm beside it, both `cmd.stderr(Stdio::null())`.

**What.** A fd 2 that names a descriptor open for *reading only* is still a
descriptor: bash gives the child that descriptor and the child's own write to it
fails. osh gives an empty stream instead, so the write succeeds and the status
differs:

```text
$ ( exec 3<in; sh -c 'echo E >&2' 2>&3 ); echo rc=$?
bash: sh: line 1: echo: write error: Bad file descriptor   →  rc=1
osh :                                                      →  rc=0
```

Only the child's *status* diverges; the diagnostic is invisible either way,
since the only fd 2 there is to report it on is the unwritable one. A child that
carries its own non-zero status through the failed write (`echo E >&2; exit 7`)
already agrees, because that status is its own.

This is the sibling of the closed-fd-2 case fixed the same day in
TD-OILS-AN-EXTERNAL-CHILD-CANNOT-BE-GIVEN-A-CLOSED-FD-0, and it is the *reason*
`StderrTarget::Discard` and `StderrTarget::Closed` are separate variants: the
closed one is now handed over as a genuine absence, the read-only one still is
not.

**Why it is not fixed with the other.** `WriteFd::ReadOnly` is a handle-less
unit variant on purpose — it records an *access mode*, not a descriptor. Giving
it a real handle to pass to the child would mean every read-only fd carrying a
clone of itself for a use it can never serve, and for `2>&0` in particular there
may be no handle at all to carry: fd 0 can be a here-doc buffer or the read end
of a pipe that the plan owns rather than a file. The honest fix is for the
`WriteFd` enum to hold the underlying object for the read-only case too and for
the child wiring to `try_clone` it, with a `Stdio::null()` fallback only where
there genuinely is no object; that is a change to the shape of `WriteFd` and to
every site that builds one.

**Impact.** Narrow. It needs an fd deliberately opened read-only *and* aimed at
an external command's fd 2 *and* that command writing a diagnostic — and it
changes only the status, never any output. Builtins already agree
(`( exec 3<in; { echo E >&2; } 2>&3 )` exits 1 in both shells), as does the fd 1
form (`cmd >&3` onto a read-only fd 3 exits 1 in both, though bash's message
comes from the child and osh's from the shell).
