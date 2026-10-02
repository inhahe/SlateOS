## TD-B-TEST-FIXTURES-SKIP-SCRATCHDIR — twelve test fixtures in three of lane B's files name a fixed temp path, so two concurrent runs delete each other's (filed by lane C, 2026-08-25) — ✅ **FIXED** 2026-08-26 in `f051d93b0`

**Not lane C's to fix** — `userspace/coreutils/src/bin/sed.rs`,
`userspace/firejail/src/main.rs` and `userspace/useradd/src/main.rs` are lane
B's. Filed as
`requests/c-b-twelve-test-fixtures-skip-scratchdir-and-collide-between-runs.md`;
recorded here because it fails the *workspace* gate, which
`TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING` makes mandatory for
every lane, so the next lane to hit it should find this rather than re-derive
it.

**Symptom.** Two `cargo test --workspace --no-fail-fast` runs on one commit,
each failing a *different* single test and passing the other:

```
thread 'tests::capital_r_takes_one_line_per_cycle_from_a_shared_position'
panicked at userspace\coreutils\src\bin\sed.rs:4728:9:
  left: "1\n2\n"   right: "1\nA\n2\nB\n"
```
```
thread 'tests::test_file_roundtrip_passwd'
panicked at userspace\useradd\src\main.rs:2775:73: failed to rename
  ...\useradd_test_7\passwd.tmp -> ...\useradd_test_7\passwd (os error 2)
```

**Observed, not inferred.** Two `cargo test --workspace` processes were alive
at once on the same worktree. One exited **0**; the other failed. Two processes,
one commit, one fixture directory, opposite results.

**Mechanism.** `userspace/scratchdir` exists precisely for this and states the
rule: uniqueness needs the pid **and** a process-wide counter, because they
cover two different axes — concurrent *runs* and concurrent *threads*. Twelve
sites cover one axis or neither: `sed.rs` 4696/4721/4822 and `firejail`
3050/3082/3094/3122/3134/3143/3159/3171 vary nothing at all, and `useradd`'s
`TestEnv::new` (1615) varies an `AtomicU32` but not the pid — so two runs both
produce `useradd_test_7`, and `TestEnv::new` opens with `remove_dir_all`, which
is why the loser sees a vanished directory rather than a wrong value.

**Three lessons, all general.**

1. **A fixture path with no per-process component is a shared mutable global.**
   It looks local because it is written inside the test. `cargo test`'s own
   thread scheduling never exposes it, so the usual "tests run in parallel"
   reasoning does not reach the case; it only appears when the suite runs twice
   at once — which on this project is normal, with three lanes and an operator
   sharing one machine.
2. **A per-process counter is the *right* fix to the *other* axis.** `useradd`'s
   comment ("Each test uses a unique temp directory to avoid interference") is
   true and still insufficient. Half a uniqueness scheme reads exactly like a
   whole one.
3. **A silent-by-design no-op in the code under test erases the evidence its own
   tests need.** `sed`'s `R` is right to say nothing about a short file — but
   that makes "the fixture was destroyed" and "the feature is broken"
   byte-identical, so the failure points at the innocent file. Where a test
   feeds a file to code that treats an empty read as a legitimate answer, assert
   the fixture immediately after writing it.

**A grep for `pid` will not audit this.** `firejail_test_nopid` is a fixed name
that contains the letters.

**Suggested fix** (lane B's call; spelled out in the request): convert all
twelve to `scratchdir::ScratchDir`, which also gets cleanup on the *failing*
path — `Drop` runs during unwind where a trailing `remove_dir_all` does not.

**Fixed** 2026-08-26 in `f051d93b0` — all twelve took `ScratchDir`, the
suggested fix rather than the `process::id()` minimum, for the cleanup-on-unwind
reason above. `useradd`'s `TestEnv` kept its `AtomicU32`; it was never wrong,
only one axis of two, and now draws both from `ScratchDir`.

The stamp is late: the commit landed on 2026-08-26 and this heading still read
**open** on 2026-08-29, found while answering lane A's
`a-b-two-restored-requests-need-a-stamp-…`. **That is the same failure this
file exists to prevent, arrived at from the other side** — a *fixed* issue left
reading open costs the next reader a re-derivation just as an *unfixed* one
left unwritten does, and it is the easier of the two to commit, because the fix
feels like the end of the work. Stamp the tracker in the commit that fixes the
bug, not in a later sweep.

Lesson 3 above — assert the fixture where the code under test treats an empty
read as a valid answer — landed separately on 2026-08-29 for the `sed` `R`
test, the one site in the twelve where that is true.
