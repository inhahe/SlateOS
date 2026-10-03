## A-KSHELL-CAPTURE-DID-NOT-NEST — `$(pipeline)` returned nothing and printed its answer on the console — ✅ FIXED 2026-08-24 (lane A)

**In short:** The kernel shell can capture a command's output — that is what
`$(…)` does, and it is also how a command typed over SSH gets its reply back to
the remote user. Capturing did not survive a pipe or a `>` redirect: running
`$(cat f | grep x)` produced an *empty* result, and the text it should have
returned was printed on the machine's own screen instead. For an SSH or telnet
user the effect was that any command containing `|` or `>` returned nothing to
them and printed their output on the host's physical console.

### What was wrong

`capture_start()` and `capture_stop()` were a bare *set*/*take* pair on one
global (`SHELL_OUTPUT`). Captures genuinely nest — `$(cat f | grep x)` is a
substitution-capture wrapped around a pipeline in which *every stage* captures —
so a caller that needed to nest had to save and restore the global by hand
around the pair.

`capture_command` did that. The four line-level plumbing functions did not:

| Function | Captures | Restored the enclosing capture? |
|---|---|---|
| `capture_command` | `$(…)`, SSH, telnet | yes, by hand |
| `execute_pipe_chain` | every pipeline stage | **no** |
| `execute_redirect` | `cmd > file` | **no** |
| `execute_input_redirect` | `cmd < file > file2` | **no** |
| `execute_heredoc` | `cmd <<EOF | …` | **no** |
| 5 × `self_test` helpers | test scaffolding | yes, by hand |

So for `$(cat f | grep x)`: `capture_command` starts capture C1; the pipeline's
first stage calls `capture_start()`, displacing C1 with no record of it, and its
`capture_stop()` leaves the global `None`; the final stage therefore runs with
*no* capture and prints to the console; `capture_command` then takes `None` and
returns empty.

The reach is what makes it serious rather than cosmetic. `capture_command` is
the entry point used by `net/ssh.rs:1759` and `net/telnet.rs:558` to run a
remotely-issued command, so this was a remote-shell correctness bug *and* put
one user's command output on another's screen.

### The fix

One invariant with six hand-written copies and four omissions is an invariant
in the wrong place, so it moved into the type. `capture_start()` now returns a
`#[must_use] struct Capture(Option<Vec<u8>>)` holding the capture it displaced,
and the bytes are reachable only through `Capture::finish(self)`, which
reinstates that capture on the way out. Forgetting the restore is no longer
something a call site can express.

The proof the discipline is now in the API rather than in the callers is that
`capture_command`'s hand-rolled prologue and epilogue **deleted**, down to
`capture_start()` … `capture.finish()`, and so did all five self-test helpers'.

There is deliberately no `Drop` impl: `finish` consumes `self`, and a kernel
panic halts rather than unwinds, so `#[must_use]` plus a consuming accessor is
the entire discipline.

### How it was found, and how it stays fixed

Not by a gate — by writing a *comment*. Self-test §12 (added the same day for
the exit-status work) carried the line "the redirect and pipe paths run their
own capture without saving this one, so nothing may be asserted about the bytes
here." That was the bug, written down and accepted as a limitation rather than
read as a defect. Boot test batch51 then printed the confirmation in the serial
log: a pipeline inside §12's capture emitted `1:alpha` onto the console.

Self-test §14 covers `capture_command` directly — plain command, two-stage
pipeline, three-stage pipeline (so a *middle* capture is exercised), output
redirect, input redirect — and asserts `SHELL_OUTPUT` is `None` afterwards, so a
capture left running cannot be missed. §12's helper now returns the captured
bytes and asserts on them, which is the assertion that fails against the old
plumbing.

**Lesson:** a comment explaining why a test cannot assert something is a bug
report that nobody filed. When the reason a test is weak is "the code under it
does not survive nesting," the weakness is the finding.
