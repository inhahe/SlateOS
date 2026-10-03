### TD-OILS-FAILGLOB-SCRIPT. `osh` `failglob` aborts the whole script, not just the current line — 2026-07-19 — ✅ FIXED 2026-07-20 (stale; the `Flow::Discard` execution-model change described below was landed after this entry was written)

**Status:** FIXED. The proper fix described below — a non-shell-exiting
`Flow::Discard` variant that unwinds only to the nearest top-level parse-unit
boundary — was implemented (see `Flow::Discard`, `exec_program_top`/`exec_items`
`skip_line` logic in `interp.rs`). The `glob_error` handlers now return
`Flow::Discard`, not `Flow::Exit(1)`. Verified byte-for-byte against bash 5.2:
a multi-line script `shopt -s failglob\necho *.nope; echo skipped\necho done`
discards only the offending line (`skipped` swallowed) and continues to `done`
(exit 0), while a single-line `-c 'shopt -s failglob; echo *.nope; echo notrun'`
discards the rest of that line (`notrun` not run, exit 1) — both matching bash.
Regression-guarded in `shopt_failglob_aborts_on_no_match`.

**Where (original diagnosis, now obsolete):** `userspace/oils/src/interp.rs` — the `glob_error` fatal-expansion
handlers returned `Flow::Exit(1)`, which `run_source` (~564) turned into
"stop executing the whole parsed program." bash, by contrast, does a
per-line top-level discard.

**What:** with `shopt -s failglob`, a glob that matches nothing is a fatal
word-expansion error. In `bash -c 'shopt -s failglob; echo *.nope; echo done'`
(one input line) bash discards the rest of the line, so `done` does *not*
run — and osh matches this exactly. But in a multi-line *script file*, bash
discards only the offending line and continues to the next one, so a later
`echo done` on its own line *does* run. osh instead aborts the entire
`run_source`, so the following lines are skipped. Reproduce: a 3-line script
`shopt -s failglob\necho *.nope\necho done` prints `done` under bash but not
under osh.

**Why deferred:** bash's behavior stems from its reader executing one
top-level command per input line and `longjmp`-ing to that per-line top
level on an expansion error. osh parses the whole source into one `Program`
and has no per-line execution boundary, so replicating the exact discard
scope needs an execution-model change. The common paths (`-c`, interactive
REPL line-at-a-time) already match bash; only multi-line scripts using
failglob diverge, which is rare.

**Proper fix (if pursued):** give `failglob` (and the other fatal
word-expansion errors) a non-shell-exiting `Flow` variant that unwinds only
to the nearest top-level list boundary, and have script execution treat each
top-level `item` in the `Program` as such a boundary (so a discard skips the
rest of the *current* item's list but resumes at the next top-level item).
