### TD-OILS-KILL-L-INTERLEAVE. `kill -l` batches all its stdout into one write, so its listing lands after every diagnostic instead of interleaving with them — RESOLVED 2026-07-27

**Resolved 2026-07-27** by the proper fix named below. A builtin's `>`/`>>`
target is now opened once and held for the builtin's whole run
(`BuiltinStdout`, installed and restored around the dispatch in
`run_builtin_body`, used by `Shell::write_redirected`), so a builtin may write
its stdout in pieces without the second piece truncating away the first.
`kill_list` and `shopt`'s two query loops write each answer where they reach it,
and their listings now keep their place among their diagnostics. Covered by
`tests/corpus/kill-options.sh`, `tests/corpus/shopt-builtin.sh` and
`a_builtin_writing_in_pieces_keeps_its_whole_output_in_a_file`; the same change
closes TD-OILS-PRINTF-ERRORDER.

**Where:** `userspace/oils/src/interp.rs` — `Shell::kill_list` (the `buf`
accumulator) and `Shell::write_bytes`.

**What.** `kill -l 9 -x 15` prints in bash

```
KILL
bash: line 1: kill: -x: invalid signal specification
TERM
```

and in osh

```
osh: line 1: kill: -x: invalid signal specification
KILL
TERM
```

Both shells exit 1 and both print the same three lines; only the order differs.

**Why.** `write_bytes` re-opens (and re-truncates) a `>` redirect on *every*
call, so a builtin that wrote its stdout in pieces would keep the last piece
only. Every builtin therefore accumulates its whole stdout and writes it once —
which necessarily puts all of it after any stderr the builtin emitted along the
way. This is the same root cause as TD-OILS-PRINTF-ERRORDER.

**Proper fix.** Install a builtin's redirects *around* the builtin (open once on
entry, restore on exit) rather than re-applying them per write, so a builtin can
stream its stdout as it goes. That is a change to how `RedirPlan` is applied to
builtins generally, not to `kill`; both this entry and TD-OILS-PRINTF-ERRORDER
close with it.

**Impact.** Cosmetic and invisible to the differential harness, which compares
stdout and stderr separately. Only visible under `2>&1` to one sink.
