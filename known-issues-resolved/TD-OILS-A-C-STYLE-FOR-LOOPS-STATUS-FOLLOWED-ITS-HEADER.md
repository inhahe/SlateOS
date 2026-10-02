### TD-OILS-A-C-STYLE-FOR-LOOPS-STATUS-FOLLOWED-ITS-HEADER. `for (( $(exit 7); 0; 0 ))` returned 7 — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_for_arith`.

**What was wrong.** Each of the three header sections is a word expansion and so
may run commands, which leave their status behind in `$?`. osh returned whatever
was left there; bash never does. `execute_arith_for_command` (execute_cmd.c)
keeps a `body_status`, starts it at `EXECUTION_SUCCESS` and writes it *only* from
`execute_command (arith_for_command->action)` — so a loop that never iterates is
a success however its header was spelled, and one that did iterate answers with
the last thing its **body** ran, even though the update section ran afterwards.

```text
for (( $(exit 7); 0; 0 )); do :; done   bash: 0   osh was: 7
for (( i=0; i<2; i=i+$(exit 7)1 )); do true; done   bash: 0   osh was: 7
```

**Fixed by** carrying a `body_status` local through `exec_for_arith` exactly as
bash does, and assigning it to `self.last_status` on the way out. Covered by
`tests/corpus/a-c-style-for-loops-status-is-its-bodys-alone.sh`.
