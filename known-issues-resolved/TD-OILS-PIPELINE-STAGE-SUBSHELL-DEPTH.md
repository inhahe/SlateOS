### TD-OILS-PIPELINE-STAGE-SUBSHELL-DEPTH. A simple-command pipeline stage counts as a subshell for `$BASH_SUBSHELL`; in bash it does not — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `clone_for_subshell` is used
for every pipeline stage, and it increments `subshell_depth`
unconditionally.

bash increments its `subshell_level` in `execute_in_subshell`, which runs
*compound* commands; a simple command in a pipeline is forked and then
executed directly, so the counter does not move for it.

```sh
echo "[$BASH_SUBSHELL]" | cat        # bash [0]   osh [1]
( echo "[$BASH_SUBSHELL]" | cat )    # bash [1]   osh [2]
{ echo "[$BASH_SUBSHELL]"; } | cat   # both [1]   — a group *is* compound
```

**Proper fix.** Make the increment belong to the *compound* subshell
constructs (`( … )`, command substitution, a braced/compound pipeline
stage, process substitution) rather than to the clone itself: pass the
depth change into `clone_for_subshell` from the caller, so a simple
command run as a pipeline stage clones without it. Note that whatever
shape is chosen must keep `$$` vs `$BASHPID` behaviour intact, which uses
the same clone.

**Fixed.** Measured against bash 5.2.37 first (29 one-liners, in
`target/dvscratch/ps1.sh`–`ps6.txt`), because the rule turned out to be
neither "every stage" nor "every compound stage":

| stage | `$BASH_SUBSHELL` |
|---|---|
| `echo $BASH_SUBSHELL \| cat`, `printf`, `q=1 echo …`, `! echo …` | +0 |
| `{ … }`, `if`, `while`, `for`, `case` | +1 |
| `( … ) \| cat` | +1 — one fork, counted once, not 2 |
| a function call | +1 |
| `eval`, `.`, `source` — also via `command`/`builtin` | +1 |
| `eval "echo $BASH_SUBSHELL" \| cat` (expanded *outside*) | +0 |
| `eval 'eval "echo \$BASH_SUBSHELL"' \| cat` | +1, not 2 |

The unifying rule is bash's, not a taxonomy of syntax: the counter moves
when the stage **re-enters the evaluator**. A plain simple command had its
words expanded *before* the fork was classified, so nothing it can print
has seen the increment; a compound command, a function body and
`eval`/`.`/`source` all evaluate more input afterwards, so they do.

Three changes carry it:

1. `Shell::subshell_depth` was split in two. It keeps its old meaning —
   *true* nesting, i.e. "am I still in the main shell environment", which
   `fatal_abort_status` and the `-c` 127-vs-1 rule consult — and the new
   `Shell::subshell_level` is what `$BASH_SUBSHELL` reads and what
   `BASH_SUBSHELL=n` rebases. `clone_for_subshell` raises both, so `( … )`,
   command substitution and `&` are unaffected.
2. `clone_for_pipeline_stage` raises only `subshell_depth` and sets
   `stage_pending_level`: the fork has happened, but whether bash would
   call it a subshell is not known until the stage's own top-level command
   is dispatched. `exec_command` claims it for a compound stage (and for a
   `Command::Subshell`, whose own clone raises the level — hence one, not
   two); `run_builtin` claims it for `eval`/`.`/`source`; `exec_simple`'s
   function branch claims it for a function call. It is *taken*, not
   tested, so only the outermost command of a stage can claim it.
3. Process substitution — a separate divergence found by the same probes
   (`cat < <(echo $BASH_SUBSHELL)`: bash 1, osh 0) — now raises
   `subshell_level` around the body in `proc_sub` and `finish_procsubs`,
   alongside the `-v`/`xtrace_level` adjustments already made there. It
   nests: `cat < <(cat < <(echo $BASH_SUBSHELL))` is 2 in both.

Covered by `tests/corpus/bash-subshell-level.sh` and by the unit tests
`bash_subshell_counts_fewer_forks_than_a_pipeline_makes` and
`a_process_substitution_is_a_subshell`.
