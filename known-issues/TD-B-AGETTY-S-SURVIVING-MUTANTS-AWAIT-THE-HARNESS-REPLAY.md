## TD-B-AGETTY-S-SURVIVING-MUTANTS-AWAIT-THE-HARNESS-REPLAY (lane B, 2026-10-16)

**Status:** OPEN

**In short:** mutation testing breaks one line of `agetty` at a time and
checks that some test notices. For 63 such breaks in the review of
0dadd1110..9454a6359 (and 139 in the review before it, on the same code)
no unit test did -- because the code broken is the part that opens the
terminal, prints the issue file and reads the login name, which only the
differential harness (`scripts/agetty-diff.sh`, against util-linux's
`agetty` in WSL) exercises, and mutation testing runs unit tests only. Until
each break has been run through the harness, nobody knows which of them it
would catch.

**Where:** `userspace/agetty/src/run.rs` (`run_inner`, `open_tty`,
`termio_init`, `auto_baud`, `Reload`, `print_issue`, `do_prompt`,
`get_logname`), `main.rs`, `issue.rs` (`SystemHost::users`), `lib.rs`
(`strerror`). The full lists: `<git-common-dir>/reviews/20261016T174550-9454a6359ddb-mutants/agetty/`
and the batch before it.

**The proper fix:** replay each surviving mutant through the harness with
`scripts/mutants-replay.py` (as was done for curses: `--tree` a scratch
worktree at the reviewed commit, the harness as the second checker), add a
harness case or a unit test for each that survives that too, and write down
why for each that cannot matter (a `pause` that only waits, a `strerror`
reached only after a failure the harness cannot cause). Long-running: start
it in the background and nothing else beside a QEMU guest.
