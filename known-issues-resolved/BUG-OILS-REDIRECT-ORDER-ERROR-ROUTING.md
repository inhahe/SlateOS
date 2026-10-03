### BUG-OILS-REDIRECT-ORDER-ERROR-ROUTING. A failing output redirect was diagnosed after the *whole* redirect list had been applied — 2026-07-27 — ✅ RESOLVED 2026-07-27

**How it was found.** The `redirection` corpus case, added to the differential
harness on the same day it was written. It was the only one of six new cases
(word-splitting, arrays, printf-formats, getopts, scoping, case-patterns,
redirection) that diverged.

**Symptom.** bash applies redirections strictly left to right against the real
fd table and aborts at the *first* failure, so both where the diagnostic goes
and which files get created depend on the order. All four rows measured against
bash 5.x:

| Case | bash | osh (before) |
|---|---|---|
| `echo A > nodir/f 2>/dev/null` | message on the terminal — `2>/dev/null` is never reached | message suppressed |
| `echo B 2>/dev/null > nodir/f` | message suppressed | message suppressed (agreed by luck) |
| `echo C > nodir/f 2>err.txt` | message on the terminal, **`err.txt` never created** | message written into `err.txt`, which was created |
| `echo D 2>err2.txt > nodir/f` | message into `err2.txt` | message into `err2.txt` (agreed by luck) |

**Root cause.** `resolve_redirects` built an order-free `RedirPlan` and left
output targets *unopened*; the failure surfaced only when the command finally
tried to write, by which point every redirect in the list — including a
trailing `2>` — had been installed. `materialize_output_files` then papered over
the "no-output builtin must still create its target" half by re-opening targets
after the fact, with the failure explicitly ignored (`let _ = open_out(…)`).

**Fix (a refactor, not a patch).**

* `resolve_redirects`'s loop body moved into `resolve_one_redirect`, so the
  `?`-heavy per-redirect logic can keep returning a bare message while the
  caller attaches state.
* New `open_output_target` performs the create/truncate **while the plan is
  being resolved**, in source order. That is both the validation and the file
  side effect, so `materialize_output_files` was deleted outright rather than
  left as a second, weaker copy of the same rule. The noclobber (`set -C`) check
  had to move *above* the open, which would otherwise truncate the very file
  noclobber exists to protect.
* `resolve_redirects` now returns `Err(Box<RedirFailure>)` carrying the plan
  built from the redirects *before* the failing one; the four call sites hand it
  to `report_redirect_failure`, which pushes the stderr sink that partial plan
  implies (file, `2>&N` write fd, `2>&1`-following-fd-1, or a capture buffer),
  emits the message through the ordinary `emit_stderr` path, and pops.
* The whole-plan dup-then-close post-pass became `reconcile_dup_then_close`,
  since it must run once after the loop rather than per redirect.

**Tests.** `failing_output_redirect_reports_through_the_fd2_of_its_moment`
(four expectations: message escapes a trailing `2>`, an earlier `2>` captures
it, `2>&1` folds it into a command substitution, and a redirect to the left of
the failure *is* applied) plus the `redirection` corpus case. 728 + 15 pass,
clippy clean, corpus 32 matched / 1 waived.

**Still order-free** (unchanged, tracked as TD-OILS14): the collapsed plan
cannot express `>&2 2>file`, where bash's `>&2` should reach the *pre*-redirect
stderr.
