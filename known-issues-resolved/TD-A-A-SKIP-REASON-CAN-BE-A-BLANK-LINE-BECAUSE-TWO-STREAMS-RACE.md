## TD-A-A-SKIP-REASON-CAN-BE-A-BLANK-LINE-BECAUSE-TWO-STREAMS-RACE (lane A) — FIXED 2026-09-03

**In short:** a build gate is allowed to *decline* — to say "I cannot check
this here" and be skipped instead of failing the build — and when it does,
the operator is shown the reason it gave. Two separate defects meant the
operator could be shown a blank reason, or worse, shown a sentence that
`run-checker.sh` had written *about* the checker, presented as the checker's
own account of itself. A gate would skip on every host and the log would
look like it had explained itself. Both halves are now fixed.

**Background: how a decline works.** `scripts/run-checker.sh` runs a checker
with its stdout and stderr merged into one log file. If the checker was
invoked with `--may-skip` and exits **2**, that is a decline; `run_checker`
sets `RUN_CHECKER_SKIPPED` and takes **the first line of the log** as
`RUN_CHECKER_SKIP_REASON`. First line, not last — that contract came from
lane B and was adopted wholesale at merge `a29a07d68`.

**Defect 1 — the two streams do not arrive in the order they were written.**
A redirected stdout is *block-buffered*; stderr is not. So a checker that
prints its reason to stdout and an explanation to stderr has the explanation
hit the file first, while the reason sits in a buffer until the process
exits. The "first line of the log" is then the wrong sentence — or, when
stderr's first write is a blank separator, no sentence at all. Observed with
`scan-unwrap.py`, whose reason went to stdout and whose detail went to
stderr, and `head -n 1` returned an empty line.

This had been fixed once before on this lane, with `PYTHONUNBUFFERED=1` on
the child. Merge `a29a07d68` adopted lane B's `--may-skip` implementation —
correctly — and the buffering fix rode along inside the file that was
replaced. It is restored at the invocation itself rather than at the call
sites, so it cannot be lost the same way a third time.

Note that the buffering bug got *worse* under the new contract without
anyone touching it. Under the old last-line rule, a buffered stdout landing
late meant the reason arrived last, which is where the reader was looking.
Under the first-line rule the same buffering hides it. A latent defect was
promoted to a live one by a change that was itself an improvement.

**Defect 2 — the guard tested the log, not the reason.** The fourth
condition for accepting a decline read `[ -s "$_rc_log" ]`: "the checker
printed something." But the claim that matters is about *the sentence this
will quote*, and the two come apart the moment the first line is blank or
whitespace. `[ -s ]` says the file has bytes; the display fallback then
substitutes the string `(it printed nothing)`; and the operator is told a
gate skipped for a reason that `run-checker.sh` invented. It now tests the
reason it is about to use, at the place the claim is made.

Both were fixed together in `a5439c918` "because each alone leaves a way
in": unbuffering without the guard still admits a checker whose genuine
first line is blank, and the guard without unbuffering still quotes the
wrong sentence when the checker used both streams.

**Regression cases** are in `scripts/test-pre-push-run-checker.py`: a
checker whose first line is blank (asserts it is not a skip, is not
reported as one, and that no invented reason reaches the operator), and a
two-stream checker run with `PYTHONUNBUFFERED` explicitly unset in the
driver — so the case fails if `run-checker.sh` ever stops setting it —
asserting the quoted reason is the one the checker wrote first.

**Standing lesson, which is why this is filed as debt and not just a fix:**
a checker that declines is trusted to describe why, and every layer between
it and the operator is an opportunity to substitute something else. Never
let the reporting layer supply a reason of its own invention. If there is
no reason, the correct output is a refusal, not a sentence.
