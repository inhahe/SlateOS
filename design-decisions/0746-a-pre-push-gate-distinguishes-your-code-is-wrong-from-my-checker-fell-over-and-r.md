## 746. A pre-push gate distinguishes "your code is wrong" from "my checker fell over", and refuses the push differently for each

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** the push hook runs eleven automatic checks, and each one asks its
checker a yes/no question by looking at the number the checker exits with. The
trouble is that a Python program that *finds a problem* and a Python program
that *crashes* both exit with the number 1, so the hook could not tell them
apart — and when a checker crashed, the hook told me, in confident detail, that
my code contained a defect. It did not. The decision is that the hook now looks
at whether the checker printed a crash report as well as at its exit number, and
says "I could not get an answer" instead of accusing the code.

### What made it visible

On 2026-09-01 a push of `lane-b` was refused by gate 8 with

```
pre-push: REFUSING to push — a diagnostic above puts a file name straight into
its message.
```

The tree had just gained a shared byte-copy body for `cp` and `mv` (§745), so a
new unquoted file name in a new diagnostic was entirely plausible, and the one
site the refusal fits — `cp.rs`'s `writeln!(job.err, "cp: {what}: {why}")`,
where the quoting happens inside `copy_bytes` rather than at the interpolation
the scanner reads — looked exactly like the culprit. Two hours went into that
theory. The tree was clean the whole time: minutes later, unchanged,
`quote-names.py --selftest` passed 56/56, `--check` reported `ok -- 0 known
sites in 0 files`, and the push went through with all ten gates green.

The cause of the original failure cannot now be established — the console log
kept only its tail, so the refusal's standing boilerplate survived and the lines
that would have named a file did not. But establishing it is no longer the
point. The post-mortem turned up a defect that is true by inspection whichever
way that particular run went.

### The defect

Every gate asked its question in this shape:

```sh
if ! "$py" "$quote" --check; then
    quote_failed=1
fi
```

`check()` returns 1 when it finds violations. An uncaught exception *also* exits
1 — that is CPython's convention, not a choice these scripts make. So the two
outcomes are the same observation, and the hook resolved the ambiguity the worst
possible way: it printed the gate's prepared refusal, which asserts a specific
defect in the operator's code, and then offered the gate's bypass —
`ALLOW_UNQUOTED_NAMES=1` — as the way past it.

Follow that advice after a crash and the push succeeds with a real gate
switched off, on the strength of a defect report that was never made. That is
worse than the gate not existing: a gate that spends its authority on verdicts
it did not reach teaches its reader to reach for the bypass, and these gates
have exactly one asset, which is that a refusal from one of them is believed.
The hook's own comments elsewhere say as much — gate 8's baseline is keyed on a
file and a count rather than a line number precisely because "a gate that cries
wolf gets bypassed."

### The decision

One helper, `run_checker <gate-name> <script> [args...]`, replaces the bare
invocation at all sixteen call sites (eleven gates, several of which run a
self-test through it first). It classifies three outcomes where there were two:

| Observed | Meaning | What happens |
|---|---|---|
| exit 0 | clean | return 0; the gate proceeds |
| exit 1, no traceback in the output | the checker reached a verdict of "found something" | return 1; the gate prints its own refusal, unchanged |
| anything else — a traceback, exit 2, a signal, an interpreter killed by the memory manager | no verdict | the hook exits 1 *from inside the helper*, with a message saying so |

The no-verdict message states plainly that this is **not** a finding against the
code, names the exit code, tells the operator not to reach for the bypass, gives
the exact command to re-run, and points at the known-issues entry for the usual
local cause (a second rustc-heavy job exhausting memory).

It exits rather than returning because there is no gate for which "the checker
fell over" is a reason to publish. Returning some third value would have made
every one of the call sites grow a `case`, and the first one written without it
would fail open.

The helper also keeps the failing checker's output in
`<git-dir>/pre-push-<gate>.log` — deleted when the gate passes, kept when it
does not, at a path derivable without having read the message that names it.
That directly answers the thing that made the 2026-09-01 failure
undiagnosable. It is inside the git dir, which for a worktree is that
worktree's own, so three lanes pushing at once do not overwrite each other's
evidence.

### Why sniffing for a traceback is not the hack it looks like

Detecting a crash by grepping the output for `^Traceback (most recent call
last):` is textual, and textual detection is usually a smell. Three things make
it right here rather than merely expedient:

1. **The marker is CPython's, not ours.** It is not a convention the checkers
   opted into and can drift from; it is what the interpreter prints for an
   uncaught exception, and has for its whole history.
2. **A false positive is safe.** If a checker ever printed that banner as part
   of a legitimate finding, the run lands in the no-verdict branch and the push
   is *still refused* — the cost is a confusing message, never a missed defect.
   The classification only ever errs toward refusing.
3. **The alternative was worse.** Making each checker exit with a distinct code
   for "I crashed" means a `try/except BaseException` footer in each of the
   eleven scripts, which is eleven places to forget, and forgetting is silent —
   the gate goes back to accusing the code. Worse, exit 2 is *already* taken:
   `quote-names.py` and `check-requests-not-deleted.py` use it for a usage
   error. The classification belongs in the one place that consumes it.

### What was rejected

- **Leave it and just remember.** Rejected because "remember that a refusal
  might be a lie" is not a property a gate can have and stay useful, and because
  the evidence that it was a lie is exactly what gets truncated away.
- **Make the checkers exit 3 on crash.** Rejected above: eleven silent
  opportunities to forget, and a collision with an existing meaning for 2.
- **Retry a crashed checker automatically.** Rejected: it converts an
  intermittent failure into an invisible one, which is the shape of the same
  hook's `check-requests-not-deleted.py` self-test — run once with output
  discarded, then re-run to display it if the first had failed, so an
  intermittent failure showed a *clean* self-test underneath the line announcing
  it had failed. That second invocation was removed as part of this change.

### Reversal

The helper is one function and the call sites are a one-token substitution
(`"$py"` → `run_checker <name>`), so reverting is mechanical.
`scripts/test-pre-push-run-checker.py` cuts `run_checker` out of the real hook
by brace matching and exercises all four outcomes, plus a rule that no gate
still invokes a checker directly — which is the regression with a future, since
the next gate added is the one likely to be written in the old shape. It runs
under `boot-test.sh`'s `scripts/test-*.py` discovery, so it is not a test
somebody has to remember to run.
