### B-PERSONALITY-TESTS-WERE-RACING-AND-AN-RAII-GUARD-MADE-THEM-LOOK-ISOLATED — 2026-08-21 — FIXED

**In short.** Two tests in the POSIX library failed at random — passing one run,
failing the next, with no code change in between. Cause: several tests all read
and write one shared setting, and `cargo test` runs tests side by side, so they
overwrote each other's setup. What made it take a while to see is that the tests
already had something that *looked* like protection and wasn't.

**Symptom.** `cargo test -p posix --target x86_64-pc-windows-gnu` intermittently:

```
---- unistd::tests::test_personality_query stdout ----
assertion `left == right` failed: Should return PER_LINUX (0)
  left: 5505024
 right: 0
```

Also `unistd::tests::test_personality_set`. The immediately preceding run of the
identical tree passed 20418/20418, which is what identified it as a race rather
than a regression — the intervening edits were doc comments only.

**Root cause.** `posix/src/unistd.rs::PERSONALITY_STATE` is one process-global
`AtomicU32`. `cargo test` runs the binary's tests on parallel threads *within one
process*, so every test touching it shares it. `unistd::tests::test_personality_query`
asserted it read `0` without ever setting it, and
`sys_personality::tests::test_phase78_combined_flags_round_trip` sets
`PER_LINUX | ADDR_NO_RANDOMIZE | MMAP_PAGE_ZERO | READ_IMPLIES_EXEC`
= `0x54_0000` = **5505024** — the exact observed value. Not a near miss; the
failing assertion printed the other test's constant.

**Why it hid — the part worth keeping.** `sys_personality`'s tests were not
naive about shared state. They had a `reset_personality()` helper *and* an RAII
`PersonalityGuard` that snapshot the value on construction and restored it on
drop, with a doc comment saying it existed "so tests don't bleed state into each
other". Both are useless here, and worse than useless because they look
sufficient:

- **Save/restore is not mutual exclusion.** It defends against *sequential*
  bleed — test A leaving a value behind for test B. It does nothing about a
  concurrent writer, which is the failure mode cargo actually produces.
- **The restore is itself a write**, so a guard dropping at the end of test A
  can clobber test B's setup mid-assertion. The mechanism intended to prevent
  interference was also a source of it.

The lesson generalises past this file: an isolation mechanism that does not
*exclude* is decoration, and its presence suppresses the question "are these
tests actually isolated?" for everyone who reads them afterwards.

**Fix.** `posix/src/unistd.rs` now exposes `PERSONALITY_TEST_LOCK` /
`lock_personality_for_test()` — same shape as the existing
`environ::ENV_TEST_LOCK`, which solved this identical problem for `ENV_STORE`
after the `wordexp::tests::tilde_*` flakes. `sys_personality`'s
`reset_personality()` now *returns the guard* (taking the lock, then resetting)
rather than returning the previous `i32`, so a caller that forgets to bind it
has nothing plausible to do with the result; `PersonalityGuard` is deleted, with
a comment where it stood saying why it must not come back. The two `unistd`
tests take the lock and establish the value they assert on instead of assuming
it.

**Generalisation.** This is the second global in this crate to need a test lock
retrofitted after producing flakes (`ENV_STORE` was the first). The audit that
this entry originally deferred **was done** the same day — see the next entry —
and found exactly one more, in `stdio`. The method that worked was a shuffled
soak, not static analysis; see there for why.
