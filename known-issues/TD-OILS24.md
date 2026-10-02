### TD-OILS24. `osh` arithmetic-error abort is command-scoped, not bash's whole-input-line longjmp — MINOR DEVIATION

**Where:** `userspace/oils/src/interp.rs` — `arith_sub` (sets the per-shell
`arith_error` flag on an `arith::eval` `Err`) and `exec_simple_inner` (checks and
clears it after word/assignment/prefix expansion, skipping the command with
status 1).

**What:** when a `$(( … ))` substitution fails to evaluate (e.g. division by
zero, `echo $((1/0))`), bash performs a `top_level` longjmp that discards **every
command sharing the current input line** — so `x=$((1/0)); echo hi` runs *neither*
the assignment nor the `echo` (execution resumes at the next input line). osh
instead aborts only the single offending simple command (or assignment) with
status 1 and continues with the following command on the same line, so
`echo $((1/0)); echo hi` still prints `hi`. Two lesser sub-deviations: (a) a pure
assignment whose value errors (`x=$((1/0))`) leaves `x` holding the fabricated
`0` rather than staying unset, because `apply_assignment` stores before the
driver observes `arith_error`; (b) the abort is only wired for the simple-command
driver — an arithmetic error inside a compound-command header expansion is not
specially discarded.

**Impact:** low. Single-command arithmetic errors (the overwhelmingly common
case) behave exactly like bash — the command is discarded, no bogus value is
produced, `$?` is 1. The divergence only shows when multiple commands share one
input line *and* an earlier one hits an arithmetic error.

**Proper fix:** introduce a `Flow::AbortLine`-style variant (or thread an
"abort to top level" signal) that `exec_program`/`run_source` propagate up to the
top-level item boundary, discarding the remaining items of the current parsed
input unit before resuming. Also expand the offending assignment's value up-front
(before storing) so an errored `x=$((1/0))` leaves `x` unset, matching bash.
