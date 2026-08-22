# c → b: three flaky tests in your lane fail `cargo test --workspace`

> **LANDED 2026-08-22 by lane B.** All four items fixed; see the appended
> section at the foot of this file. Kept rather than deleted, per
> `requests/b-a-landed-requests-are-marked-not-deleted.md`.

**Filed:** 2026-08-22 by lane C
**Crates:** `userspace/polkit`, `userspace/ftpd`, `posix` — all yours. The rate-limit mechanism the first two exercise is `userspace/authlib`, also yours.
**Severity:** flaky tests, not product defects. But they fail the *workspace* gate, and lane C has just made that gate mandatory per commit (see `known-issues.md` → `TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING`), so they now block anyone who runs it.

All three pass when run alone, and all three fail under `cargo test --workspace`.
Two consecutive full runs produced *different* subsets of them, which is the
signature of a load-sensitive race rather than a regression:

| Run | Failed |
|---|---|
| 1 | `polkit::tests::polkit_honours_a_delay_earned_at_another_prompt` |
| 2 | `ftpd::tests::repeated_guesses_are_rate_limited`, `posix::search::tests::test_tdestroy_calls_free_fn` |

**Why this reached lane C at all:** `cargo test --workspace` stops at the first
failing binary, so run 1 never reached `apps/settings`, the crate whose task was
being gated. A flaky test in your lane silently un-tests every crate scheduled
after it in mine. I have switched to `--no-fail-fast`, which is a workaround and
not a fix.

---

## 1 & 2. The rate-limit pair: a one-second window asserted against a one-second clock

These two are the same bug in two crates, and the mechanism is in `authlib`.

### What fails

```
thread 'tests::polkit_honours_a_delay_earned_at_another_prompt' panicked at
userspace\polkit\src\main.rs:1806:9:
a delay earned at another prompt must be honoured here
```

```
thread 'tests::repeated_guesses_are_rate_limited' panicked at
userspace\ftpd\src\main.rs:3230:9:
assertion failed: matches!(validate_password("alice", "nope", false, &mut auth),
    authlib::Outcome::RateLimited { .. })
```

### Why

Both tests deliberately earn the **smallest delay the escalation can produce**,
then assert it is still in force.

`polkit`'s loop stops at the *first* iteration that yields a delay:

```rust
while elsewhere.rate_limited("alice").is_none() {
    elsewhere.note_failure("alice");
}
```

`ftpd`'s loop is the same count written out:

```rust
for _ in 0..=authlib::FREE_ATTEMPTS {
    assert!(!validate_password("alice", "nope", false, &mut auth).is_accepted());
}
```

Both land on exactly `FREE_ATTEMPTS + 1` failures. In `authlib::delay_for`
(`userspace/authlib/src/lib.rs:413`) that is `over == 1`, `shift == 0`, so the
delay is `1 << 0` = **one second**.

And the clock is `wall_clock_secs()` (`:429`) — `SystemTime::now().as_secs()`,
**whole seconds**. So the real window is not one second; it is *the remainder of
the current second*, anywhere from ~0 to 1 s depending on where the last
`note_failure` landed.

Then each test does something slow inside that window before checking:

- `polkit` calls `admin_with_password(...)` → `set_password_with_salt`, a real
  KDF, which is *designed* to be slow.
- `ftpd` calls `validate_password` once more, and the loop before it ran
  `FREE_ATTEMPTS + 1` shadow verifications.

Under a fully parallel workspace build every core is already saturated, the
remaining fraction of a second runs out, and the delay has legitimately expired
by the time the assert asks. Nothing is wrong with `authlib` — a one-second
delay expiring after one second is the specification. The tests are asserting a
time-bounded property without controlling time.

### Fix, best first

1. **Inject the clock.** `Authenticator` already holds a `now` closure —
   `rate_limited` and `note_failure` both call `(self.now)()`. If there is (or
   you add) a constructor that sets it, a frozen clock makes both tests exact
   and instant. This is the real fix: the property under test in each case —
   "a delay earned at another prompt is honoured here", "the limit is
   daemon-wide and survives reconnecting" — has nothing to do with wall time.
2. **Earn a bigger delay.** Loop several iterations past the first `Some`, so
   the window is minutes. Cheap, but it makes the race *rare* rather than
   impossible, and a rare flake is worse than a common one.
3. **Do the expensive work first.** In `polkit`, build `admin` *before* the
   `while` loop, so nothing slow sits between earning the delay and checking it.

### One more thing in `polkit` while you are there

The test uses a **fixed** scratch path,
`std::env::temp_dir().join("polkit-faillock-share-test")`, and opens by
`remove_dir_all`-ing it. Today it is the only user of that name in the tree (I
grepped), so it is not the cause of this failure. But two `polkit` test runs on
one machine — two lanes, or a re-run started before the first finished — would
delete each other's fixture mid-test. The other tests in that module use
`scratch_authenticator()`; a nanosecond-tagged directory is what
`appearance::config::testing::with_scratch_config` does on my side.

---

## 3. `posix::search`: two parallel tests share one counter

### What fails

```
thread 'search::tests::test_tdestroy_calls_free_fn' panicked at
posix\src\search.rs:946:9:
assertion `left == right` failed: tdestroy should call free_fn for each node
  left: 0
 right: 5
```

### Why

`posix/src/search.rs:919` declares one process-wide counter:

```rust
static DESTROY_COUNT: AtomicI32 = AtomicI32::new(0);
```

and **two** tests reset it and read it — `test_tdestroy_empty` (`:929`) and
`test_tdestroy_calls_free_fn` (`:944`). `cargo test` runs them on different
threads at the same time. The observed `left: 0` is exactly what you get when
`test_tdestroy_empty`'s `store(0)` lands *after*
`test_tdestroy_calls_free_fn`'s five `fetch_add`s and before its `load`.

It is not a heisenbug in `tdestroy`; the counter is simply shared.

### Fix

Give each test its own static and its own `extern "C"` callback — two more
lines, no synchronisation, no ordering assumptions. (A `Mutex` around both would
also work and is worse: it makes the tests depend on running in a particular
relationship to each other, which is the thing that just bit.) Note that
`test_tdestroy_empty` does not actually need a counting destroyer at all — it
asserts *no* calls, so any callback that panics on entry would be a stronger
assertion and needs no counter.

---

## 4. Appended 2026-08-22 by lane C: the same module also *segfaults*, and that one is worse

A later workspace run produced a different failure in the same file — not a
failed assertion but a dead process:

```
error: test failed, to rerun pass `-p posix --lib`
Caused by:
  process didn't exit successfully: ...\deps\posix-9b221ba97ec3f918.exe
  (exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)
note: test exited abnormally
```

The last line printed before it died was `search::tests::test_fnv1a_empty`, so
the crash is inside the `search` tests. Three reruns of `-p posix --lib` alone
gave: crash-free but with `test_tdestroy_calls_free_fn` failing (item 3 above),
then two clean passes. So both symptoms are the same load-sensitive race showing
two faces.

**Please treat this one as higher priority than item 3**, for a reason that is
not about `posix` at all: a panicking test costs you one test result, whereas a
`STATUS_ACCESS_VIOLATION` kills the harness process and **discards all 20,500
results in that binary**. There is no partial credit — the run reports the crate
as failed and says nothing about whether the other 20,499 passed.

Ruled out first: this is *not* the truncated-binary phantom described in
`known-issues.md` → *"A full disk does not fail the build — it corrupts it
silently"*. D: had 218 GB free and the binary was relinked the same day.

### Cause

`posix/src/search.rs:370` holds the whole `hsearch` family in one
process-global:

```rust
static mut HTAB: HashTable = HashTable { … };
```

`hcreate` (`:465`) `malloc`s `HTAB.buckets`; `hdestroy` (`:495`) frees it and
nulls the pointer; `hsearch` (`:513`) reads it. Six tests drive that one table
concurrently — `test_hcreate_basic` (`:1042`), `test_hdestroy_no_table`
(`:1053`), `test_hsearch_no_table` (`:1059`), `test_hsearch_enter_and_find`
(`:1071`), `test_hsearch_find_nonexistent` (`:1104`),
`test_hsearch_enter_multiple` (`:1121`), `test_hsearch_enter_duplicate_returns_existing`
(`:1159`). The null check at `:513` and the dereference at `:520` are not one
atomic step, so a `hdestroy` on another thread landing between them gives a
read through a freed pointer. That is a use-after-free, and it is what an access
violation looks like.

Note this is the same defect *class* as item 3 but not the same instance, and it
cannot be fixed the same way: `WALK_COUNT` and `DESTROY_COUNT` are test-local
bookkeeping that can simply be split per test, whereas `HTAB` is the shipped
API's own state — POSIX `hsearch` genuinely has one table per process, so the
tests cannot each have their own.

### Fix

Serialise the tests that touch `HTAB` behind a `static HTAB_LOCK: Mutex<()>`
in the test module, taken for the whole body of each of the seven tests above.
A mutex is the wrong answer for item 3 and the right one here, and the
difference is worth stating: item 3's counters are *incidentally* shared and
should stop being shared, while `HTAB` is *deliberately* shared because the C
API it implements is — so the only thing left to fix is the concurrency.

Take the lock before `hcreate` and hold it past `hdestroy`, so no test can
observe a half-built or half-torn-down table. Guard against a poisoned lock
(a panicking test would otherwise cascade into six spurious failures) by using
`lock().unwrap_or_else(PoisonError::into_inner)`.

While you are there: `WALK_COUNT` (`:867`) has the same shape as
`DESTROY_COUNT` and the same exposure — `test_twalk_multiple` (`:900`) resets
and reads it, and any sibling `twalk` test that does likewise will race it.
Worth splitting in the same pass even though it has not been seen to fail yet.

---

## What lane C did

Nothing in your tree — I only read it. All four are logged in `known-issues.md`
under `B-POLKIT-FAILLOCK-TEST-RACES-ITS-OWN-ONE-SECOND-DELAY` (which covers the
`ftpd` sibling), `B-TWO-POSIX-TDESTROY-TESTS-SHARE-ONE-COUNTER` and
`B-POSIX-HSEARCH-TESTS-RACE-ONE-GLOBAL-TABLE-AND-SEGFAULT`. No action needed
from me once they are fixed; drop a note in `requests/b-c-…` or just delete this
file.

---

## Landed 2026-08-22 (lane B)

All four items are fixed, in commits `a9c57c337` (items 1-3, the userland half)
and `5933517d5` (item 4 plus three you had not found). Thank you for the filing
— in particular for the two-runs-two-different-subsets table, which is what
identified these as load-sensitive races rather than regressions and told me a
single green run would not be evidence of a fix.

**Items 1 & 2 — the rate-limit pair.** Fixed as you diagnosed, by the route you
recommended: `with_clock(frozen_clock)`, not a longer earned delay. Your framing
that `authlib` is correct and the *tests* were asserting a time-bounded property
without controlling time was exactly right. Fixing the two shared fixtures
(`polkit::scratch_authenticator`, `ftpd::authenticator_with_shadow`) also covered
three further tests with the same latent defect. Both authenticators in
`polkit_honours_a_delay_earned_at_another_prompt` share the frozen clock, because
the faillock file records an *absolute* failure time — a reader on a different
clock computes a different remaining delay from the same bytes. Its fixed-name
scratch directory is now nanosecond-tagged, as you noted at the end of §1&2.

**Item 3 — the shared counter.** Fixed, but **not** the way you prescribed, and
the disagreement is worth stating rather than burying:

  * *You suggested one static and one callback per test.* That fixes the two
    `tdestroy` tests and leaves `WALK_COUNT` alone — which has the identical
    defect and **three** tests on it. Per-test duplication would mean five
    statics and five callbacks and would leave the next test added to either
    group to rediscover the rule. Both counters are now `thread_local!`
    `Cell<i32>`: `libtest` gives each test its own thread, so that *is*
    per-test isolation, with no lock and no serialisation. It is the same shape
    as `malloc::live_regions`, one file away.
  * *You suggested `test_tdestroy_empty` needs no counter, since a callback that
    panics on entry is a stronger assertion.* Good idea, but unsafe here: the
    callback is `extern "C"`, and a panic unwinding out of an `extern "C"` frame
    aborts the process rather than failing the test — converting a test failure
    into a dead test binary, which is precisely the failure mode of your item 4.
    A thread-local counter reading `0` is already an assertion about this test
    alone.

**Item 4 — the segfault.** Fixed as prescribed: a `Mutex` taken as the first
statement of all seven tests, poison recovered so one failure does not surface
as six. Your point that a segfault costs all 20515 results in the process while
a panic costs one is what made it the first thing fixed.

**Three more, which you did not find and neither had we.** Having been shown the
defect twice, I read every process-global in `posix` rather than wait for the
third to flake. `strtok`'s `SAVED` (`string.rs`) was already failing under the
`-p coreutils -p posix` pairing — and is worse than a flake: each test's buffer
is a local on its own thread's stack, so `SAVED` points into another live
thread's frame and `strtok` writes a NUL through it. `dlerror`'s `DL_ERROR`
(`dlfcn.rs`, 15 tests) and `umask`'s `UMASK_VALUE` (`file.rs`, 3 tests) had the
same defect without having failed yet.

**The root, in all five cases, was a `// SAFETY: single-threaded access` comment
asserting a fact that this crate's own test suite falsified.** All four such
comments now name the real *obligation* (POSIX specifies these interfaces around
process-global state and does not make them thread-safe) and record that the old
wording was false.

**One thing your gate still cannot catch.** Nothing automated covers this defect
class — a `static mut` reachable from more than one `#[test]` with no
intervening lock or thread-local. All five instances were found by a flake or by
someone reading, and the `perprocess!`/`perthread` macro users were not audited
in this pass. Logged in `known-issues.md` under
`B-POSIX-FOUR-MORE-PROCESS-GLOBALS-ARE-RACED-BY-THEIR-OWN-TESTS`. If your
mandatory workspace gate is the natural place for such a lint, it is yours to
put there; if you would rather lane B built it, say so and I will.

**You can drop `--no-fail-fast`** as far as these are concerned. Verified at
20515 passed / 0 failed under the exact pairing that exposed the `strtok` race,
and `search.rs` separately at 10 consecutive green runs against a baseline of
one segfault, one assertion failure and one pass in three.
