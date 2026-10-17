## TD-B-CHATTR-S-MUTANTS-AWAIT-THE-WSL-REPLAY (lane B, 2026-10-17)

**Status:** OPEN

**In short:** mutation testing breaks one line of a program at a time and
checks that some test notices. In the review of `31ce92405..d89be2f7a` (the
port of e2fsprogs' `chattr` and `lsattr`) it broke 196 lines and no test
noticed any of them -- not because nothing would, but because mutation
testing runs the unit tests on the Windows host, where every line it broke
is compiled away: `chattr`'s modules are `#[cfg(unix)]` (they open files and
call `ioctl`), and so is `libcall::inode`. What checks them is
`scripts/chattr-diff.sh`, against Ubuntu's programs in WSL, which mutation
testing does not run.

**Where:** `userspace/chattr/src/{main,chattr,lsattr,e2p}.rs` (183 mutants)
and `libcall/src/inode.rs` (13). The list:
`<git-common-dir>/reviews/20261017T001248-d89be2f7a46d-mutants/`. Since then
the flag tables and `print_flags` moved to `flags.rs` (ff9c34c51), which
builds on every host and has tests there, so those mutants are now within
mutation testing's reach.

**The proper fix:** replay them through the harness with
`scripts/mutants-replay.py` -- `--tree` a scratch worktree at `d89be2f7a`
(lane B's `os-lane-b-mutants` will do), `scripts/chattr-diff.sh` as the
harness -- then add a harness case or a WSL unit test for each that survives,
and write down why for each that cannot matter (a `close` whose failure is
ignored upstream too, `com_err`'s carriage return on a terminal the harness
has none of). About two minutes a mutant: start it in the background, beside
no QEMU guest, and not just before the move to the new PC.
