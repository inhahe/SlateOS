## B-POSIX-HSEARCH-TESTS-RACE-ONE-GLOBAL-TABLE-AND-SEGFAULT (found by lane C, 2026-08-22 — lane B's crate) — FIXED 2026-08-22

**In short:** Seven tests in `posix/src/search.rs` all drive one process-wide
hash table at the same time, on different threads. One test frees the table
while another is reading it, so the reader dereferences freed memory and the
whole test process dies. Because it dies rather than failing, all 20,500 test
results in that binary are lost, not just the one.

**Where.** `posix/src/search.rs:370` — `static mut HTAB: HashTable`. `hcreate`
(`:465`) allocates `HTAB.buckets`, `hdestroy` (`:495`) frees it and nulls the
pointer, `hsearch` (`:513`) checks it for null and then dereferences it at
`:520`. Those last two are not one atomic step. The seven concurrent tests are
`test_hcreate_basic` (`:1042`), `test_hdestroy_no_table` (`:1053`),
`test_hsearch_no_table` (`:1059`), `test_hsearch_enter_and_find` (`:1071`),
`test_hsearch_find_nonexistent` (`:1104`), `test_hsearch_enter_multiple`
(`:1121`) and `test_hsearch_enter_duplicate_returns_existing` (`:1159`).

**Observed:**

```
error: test failed, to rerun pass `-p posix --lib`
Caused by:
  process didn't exit successfully: ...\deps\posix-9b221ba97ec3f918.exe
  (exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)
```

The last test to print before the process died was `search::tests::test_fnv1a_empty`.

**Why this matters more than its sibling above.** A panicking test costs one
result; a segfault costs the whole binary. `cargo test` reports the crate as
failed and can say nothing about the other 20,499 tests, so a green-looking
workspace run and a crashed one are equally uninformative about `posix`.

**Ruled out.** Not the truncated-artifact phantom described under *"A full disk
does not fail the build — it corrupts it silently"*: D: had 218 GB free and the
binary had been relinked the same day. Three isolated reruns of `-p posix --lib`
gave one `test_tdestroy_calls_free_fn` failure and two clean passes, which is
the signature of a load-sensitive race.

**Fix.** A `static HTAB_LOCK: Mutex<()>` in the test module, held for the whole
body of each of the seven tests — taken before `hcreate` and held past
`hdestroy`, so no test can observe a half-built or half-freed table. Use
`lock().unwrap_or_else(PoisonError::into_inner)` so one panicking test does not
cascade into six poisoned-lock failures.

Note this is deliberately the *opposite* prescription from
`B-TWO-POSIX-TDESTROY-TESTS-SHARE-ONE-COUNTER` directly above, and the
difference is the point: those counters are *incidentally* shared and should
simply stop being shared, whereas `HTAB` is *deliberately* shared because the
POSIX `hsearch` API it implements genuinely has one table per process. You
cannot give each test its own, so the only thing left to fix is the concurrency.

**Also worth doing in the same pass.** `WALK_COUNT` (`:867`) has the same shape
and exposure as `DESTROY_COUNT` — `test_twalk_multiple` (`:900`) resets and
reads it, and any sibling `twalk` test that does the same will race it. It has
not been seen to fail yet.

### Fixed 2026-08-22 (lane B) — as prescribed, plus the SAFETY comments

`HTAB_TEST_LOCK` is a `std::sync::Mutex<()>` in `search.rs`'s test module, taken
as the **first statement** of all seven tests so it is held from before
`hcreate`/`hdestroy` to past the end of the body. Poison is recovered with
`unwrap_or_else(PoisonError::into_inner)`, so one failing test reports one
failure instead of six poisoned-lock failures burying the cause. `WALK_COUNT`
was done in the same pass, by the different route the entry above explains.

**The production `// SAFETY: single-threaded access` comments were the real
root, and all three are rewritten.** They asserted a *fact* that nothing
established. What is actually true is an *obligation*: POSIX defines the
`hsearch` family around one process-global table and does not make it
thread-safe (`hsearch_r` is the reentrant form), so serialising calls is the
caller's job. The tests were simply a caller that did not do it. The new
comments name the obligation, say who discharges it, and record that the old
wording was false — because a SAFETY comment stating an unchecked fact is worse
than none: it tells the next reader the question has been considered.

**Verified** by 10 consecutive `cargo test -p posix --lib` runs, against a
baseline of 3 pre-fix runs that gave one segfault, one assertion failure and one
pass. A single green run would not have been evidence for a load-sensitive race.

**Filed to the owning lane** as item 4 of
`requests/c-b-three-flaky-tests-fail-the-workspace-gate.md`. Lane C has not
touched the file.
