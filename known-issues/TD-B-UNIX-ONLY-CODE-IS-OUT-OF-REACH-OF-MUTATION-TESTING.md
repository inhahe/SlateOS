## TD-B-UNIX-ONLY-CODE-IS-OUT-OF-REACH-OF-MUTATION-TESTING (lane B, 2026-10-17)

**Status:** OPEN

**In short:** mutation testing (breaking one line at a time to see whether
any test notices) runs the unit tests on the Windows host. A program that
only exists on Unix -- because it opens devices, sockets or extended
attributes through the C library -- is compiled away there, so every line
of it counts as "no test noticed", whether or not one would. Such programs
are measured instead by a harness against Ubuntu's in WSL, which mutation
testing does not run. `chattr` is the first case and has its own entry
(TD-B-CHATTR-S-MUTANTS-AWAIT-THE-WSL-REPLAY); this one is the rest.

**Where:**

| Crate | What is Unix-only | What checks it |
|---|---|---|
| `userspace/rfkill` | the device loops, `rfkill_block_on`, `rfkill_event`, `fill_table_row` (`tests.rs`'s `FakeDevice` tests are `#[cfg(unix)]`) | `scripts/rfkill-diff.sh`; the unit tests under WSL |
| `userspace/ulallio` | `read_all`, `write_all` (libcall answers `ENOSYS` off Unix) | its unit tests under WSL |
| `libcall/src/fd.rs` | `poll` (6 mutants survived the review of `b50f04266..07e9cfcd0`) | its unit test under WSL; `rfkill-diff.sh`'s `event` cases |
| `userspace/curses/src/lib.rs` | not Unix-only, but as far out of reach: the wrappers around the one process-wide screen (`use_legacy_coding`, `tigetstr`), which no unit test may install beside the others | `Screen`'s own methods' tests; `curses-diff.sh` |

In the review of `b50f04266..07e9cfcd0` mutation testing found 153 mutants
no test caught: 135 in `rfkill`, the rest in the rows above and in chattr.
Host tests were added for the ones in code every host builds (the
timestamp's zone, the option switch and its status, the words of the
states, the program's name); the next batch's mutation run, which covers
rfkill's changed lines again, says how many they catch. The device paths
are this entry's.

**The proper fix:** either mutation testing builds these crates for
`x86_64-unknown-linux-gnu` in WSL and runs their tests there (a change to
`scripts/review-batch.py`, which is not lane B's), or each batch that
touches them is replayed through its harness with
`scripts/mutants-replay.py`, as the chattr entry describes. Until one of
those is done, a surviving mutant in these files says nothing about the
tests -- but neither does a missing one.
