### TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP. `>&-` / `<&-` on fd 0, 1 or 2 of a *command* closes nothing — ✅ RESOLVED (all three fds) — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `resolve_dup_out` / `resolve_dup_in`,
the `DupWord::Close` arm. For a redirect on a command (not `exec`) the close is
recorded by removing fd from `open_fds` / `open_write_fds` / `coproc_read_fds`;
fds 0–2 do not live in those tables, so the removal finds nothing and the
command runs with its stream still attached.

**Reproduce** (measured 2026-08-01 against the reference bash; `rc` and the
diagnostic are bash's, the osh column is what it did *before* the fixes below.
Every row now matches bash, the external-command ones included since
2026-08-06 — see the note at the end of each fd's section):

| | bash | osh |
|---|---|---|
| `echo hi >&-` | rc 1, `echo: write error: Bad file descriptor` | prints `hi`, rc 0 |
| `printf 'x\n' >&-` | rc 1, `printf: write error: Bad file descriptor` | prints `x`, rc 0 |
| `read -r l <&-` | rc 1, `read: read error: 0: Bad file descriptor` | reads the real stdin |
| `{ echo A; } >&-` | rc 1, `echo: write error: …` | prints `A`, rc 0 |
| `{ read -r l; } <&-` | rc 1, `read: read error: 0: …` | rc 1, silent |
| `{ echo Q >&0; } <&-` | rc 1, `0: Bad file descriptor` (a *redirect* error) | rc 1, `echo: write error: …` |
| `echo hi >&- >&-` | rc 1, `echo: write error: …` | prints `hi`, rc 0 |
| `echo hi >&- 1>f1` | rc 0, `f1=[hi]` | same |
| `{ echo Z; } >&- 3>&1` | rc 1, `1: Bad file descriptor` | prints `Z`, rc 0 |
| `echo hi >&2 2>&-` | rc 0, `hi` on the *original* fd 2 | same |
| external `cmd >&-` | rc 1, the child's own `cmd: write error: …` | n/a on this host |

Two shapes of failure, then: a *write/read* error named against the builtin when
the command runs with the stream gone, and a *redirect* error naming the number
when a later redirect in the same list tries to dup from what an earlier one
closed. The last is order-sensitive — `>&- 3>&1` fails, `3>&1 >&-` does not —
and so runs into the order-free `RedirPlan` (see `TD-OILS14` and
`Shell::reconcile_dup_then_close`, which today drops a transient close of any fd
the plan still dups from). Reopening a closed descriptor in the same list is
fine and must stay fine.

**Impact.** The idiom that runs a command with a stream deliberately taken away
— `cmd >&-` to prove it writes nothing, `cmd <&-` to prove it reads nothing —
silently does the opposite of what it says. Both statuses and both streams
differ from bash.

`exec`'s own `M>&-` was affected too, contrary to what this entry first claimed:
`apply_persistent_dup_out`'s fd 1/2 arms set `exec_stdout`/`exec_stderr` to
`None`, and `None` does not mean *closed* — it means *inherit the real one*. So
`exec 1>&-; echo hi` printed `hi` with status 0 where bash gives
`echo: write error: Bad file descriptor` and status 1. A persistent close was
therefore not a close but a **reset to the terminal**, which is its opposite.

**Related.** `TD-OILS-FD0-WRITE`'s residual divergence 2 (`{ echo Q >&0; } <&-`)
is the same gap seen from the write side; fixing this subsumed it.

**Proper fix** (what was done — see the three sections below). Give the transient
redirect plan a *closed* state for fds 0–2,
distinct from "absent, so inherit the real one", so that a builtin resolving its
stdin/stdout/stderr through the plan gets `Bad file descriptor` rather than the
inherited handle — and so an external child is handed a closed descriptor. The
write side already knows how to report this (`TD-OILS-WRITE-ERROR-REPORTING`
gives `echo: write error: …` named against the builtin); the read side needs the
matching `read: read error: N: …` shape. Then extend
`tests/corpus/dup-word-that-is-not-a-descriptor.sh`, which deliberately avoids
`-` today for exactly this reason.

**Fixed so far — fd 1 (2026-08-01).** `WriteFd::Closed` is that closed state: a
descriptor that is not open at all, distinct both from "no binding" (inherit the
real fd 1) and from `WriteFd::ReadOnly` (open, but with no write half). The
transient side carries it as `RedirPlan::stdout_closed`, the persistent side as
`exec_stdout = Some(WriteFd::Closed)`, and every write path answers it with
`EBADF`. Order between fd 1's destinations now runs through one
`RedirPlan::clear_stdout`, so `echo hi >&- 1>f1` still writes the file while
`echo hi 1>f1 >&-` fails — last writer wins either way. A closed fd 1 is also not
a descriptor to dup *from*: `exec 1>&-; exec 3>&1` reports `1: Bad file
descriptor` at the `exec`.

Two divergences were knowingly left in the fd 1 case:

* **External children.** bash handed the child the closed descriptor and let the
  *child's* write fail (`cmd: write error: …`); `Stdio` has no closed form and a
  forged one looked unportable, so the shell refused the redirect instead —
  `1: Bad file descriptor`, same status (1) and same stream, different wording.
  **Closed 2026-08-06** by `ClosedStd`, the same mechanism that closed the read
  side; see TD-OILS-AN-EXTERNAL-CHILD-CANNOT-BE-GIVEN-A-CLOSED-FD-0.
* **A dup *from* fd 1 later in the list that closed it** — `{ echo Z; } >&- 3>&1`,
  `$( { echo E; } >&- 2>&1 )`. bash fails both with `1: Bad file descriptor`, osh
  succeeds: `install_extra_fds` (and the `2>&1` resolution) run before the scoped
  `exec_stdout` override, and the order-free plan cannot tell `>&- 3>&1` from
  `3>&1 >&-`. Same class as `TD-OILS14` / `reconcile_dup_then_close`.

A neighbouring bug fell out of this and is fixed with it: `alias_write_fd`
answered `exec 3>&1` with the *ambient* capture/pipe sink without first asking
whether a persistent `exec` had rebound fd 1 underneath it, so
`x=$( exec 1>f; exec 3>&1; echo hi >&3 )` put `hi` in the substitution's buffer
instead of in `f`. It now consults `exec_stdout_shadowing` first.

Covered by `tests/corpus/redirect-close-of-stdout.sh` (which states in its header
which forms it deliberately leaves out, and why) plus five unit tests in
`interp.rs`.

**Fixed next — fd 2 (2026-08-01).** `RedirPlan::stderr_closed` and
`exec_stderr = Some(WriteFd::Closed)` are the fd 2 halves of the same pair, with
`RedirPlan::clear_stderr` giving fd 2's destinations the same one-place
last-writer-wins ordering fd 1 got. What is *visible* differs, though, because a
failed write to fd 2 has nowhere to be reported: closing fd 2 does not turn the
diagnostic into an error, it simply **drops** it, and the status stays exactly
what it would have been (`cd /nosuchdir 2>&-` still exits 1, silently). So a
closed fd 2 resolved to the existing `StderrTarget::Discard` — no new stderr
variant seemed to be needed.

The interesting part is what other redirects can then do with fd 2. `1>&2` is a
*dup* performed at redirect setup, so with fd 2 gone it fails outright: status 1,
nothing written, and its own `2: Bad file descriptor` invisible for the same
reason everything else is. Measurement showed bash answers a *read-only* fd 2
(`2>&0` under a plain `< file`) identically wherever the shell itself does the
writing, which is why `Discard` stood for "no write half" uniformly at first.
`exec 3>&2` likewise fails, while a `>&2` taken *before* the close copies fd 2
while it is still open, so `echo hi >&2 2>&-` writes to the original stderr and
exits 0.

External children were given `Stdio::null()`, on the reasoning that dropping is
the correct answer here rather than a substitute for one. That was half right:
dropping *is* correct, but an empty stream drops by taking the bytes, and taking
them means the child's write succeeds. **Corrected 2026-08-06** — `sh -c 'echo E
>&2' 2>&-` exits 1 in bash and did exit 0 here, so `StderrTarget::Closed` was
split out of `Discard` after all and the child is handed a genuine absence via
`ClosedStd`. `Discard` kept the read-only meaning. The `1>&2` refusal above is
now that variant's doing rather than a `WriteFd` check. See
TD-OILS-AN-EXTERNAL-CHILD-CANNOT-BE-GIVEN-A-CLOSED-FD-0 for the mechanism.

`Discard` was wrong in the same way for the same reason, and was corrected the
same day: it now carries the read-only description so the child can be handed
*that*, whose write fails as bash's child's does. See
TD-OILS-A-READ-ONLY-FD-2-IS-HANDED-TO-AN-EXTERNAL-CHILD-AS-AN-EMPTY-STREAM.

The same divergence fd 1 has remains: `2>&- >&2` (bash fails the dup, osh does
not) is the order-free-plan limit, identical in kind to `>&- 3>&1`.

Covered by `tests/corpus/redirect-close-of-stderr.sh` plus three unit tests.

**Fixed last — fd 0 (2026-08-01).** fd 0 needed no new plan field at all. Its
plan slot already holds a descriptor (`RedirPlan::stdin: Option<InputFd>`) rather
than a path, so the closed state fits *inside* it as a third `InputSrc` variant,
`InputSrc::Closed`, whose `read`/`fill_buf` answer `EBADF`. Because
`InputFd = Arc<Mutex<InputSrc>>`, that reaches every reader in the interpreter
with no type change anywhere: a `<&-` simply records itself as a source like any
other, and `RedirPlan::clear_stdin` gives fd 0 the same one-place
last-writer-wins ordering fds 1 and 2 got (`<&- <in` reads the file, `<in <&-`
does not). The persistent side is `exec_stdin = Some(closed_input())` — where it
used to be `None`, which means *inherit the real stdin* and made `exec 0<&-` a
reset to the terminal, the same inversion fds 1 and 2 had.

The read side's error shape came with it. `read_record` / `read_one_line` /
`Shell::read_line` / `Shell::read_record_input` returned `Option`, folding "could
not read" into "end of input"; they now return `io::Result<Option<…>>`, and the
distinction is worth the churn because bash makes it visible twice over — EOF
*assigns* the empty record it did not find while a read error assigns nothing
(`l=keep; read -r l <&-` leaves `l` as `keep`), and EOF is silent while a read
error prints `read: read error: 0: Bad file descriptor`. `mapfile` is bash's
exception and stays one: an unreadable descriptor is an empty array, status 0 and
no message. `select` reads through the same path, ends its loop rather than
spinning, and uses bash's bare `read error:` wording without the builtin's
`read:` prefix.

On the write side a closed fd 0 is now `None` from `Shell::stdin_write_fd`, i.e.
genuinely no such descriptor, where a read-only one is `WriteFd::ReadOnly`. That
is the distinction `{ echo Q >&0; } <&-` turns on — bash fails it at the
*redirect* (`0: Bad file descriptor`) and `{ echo Q >&0; } < file` at the *write*
(`echo: write error: …`) — and it resolves `TD-OILS-FD0-WRITE`'s residual
divergence 2. `exec 0<&-; exec 3<&0` and `exec 3>&0` likewise fail at the `exec`,
since a descriptor that is not there cannot be duplicated.

One divergence was knowingly left, the fd 1 trade again: an external `cmd <&-`
got `Stdio::null()` (`ChildIn::Closed`), so it saw an empty stdin rather than
`EBADF` and `cat <&-` exited 0 where bash's `cat` exits 1 complaining about the
descriptor. **Closed 2026-08-06** — `ClosedStd` hands the child the missing
descriptor after all, without the raw `CreateProcess` that looked necessary;
see TD-OILS-AN-EXTERNAL-CHILD-CANNOT-BE-GIVEN-A-CLOSED-FD-0. The equivalents on
the *write* side (an external `cmd >&-` and `cmd 2>&-`) were closed by the same
mechanism on the same day, so all three descriptors now agree with bash; that
entry tracks the set.

Covered by `tests/corpus/redirect-close-of-stdin.sh` plus four unit tests.
