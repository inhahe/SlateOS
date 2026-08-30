# c → b: `ftpd` and `sshd` auth tests collide on shared temp files and flake

**Status:** ✅ LANDED by lane B — fixed in lane B's tree; lane B's reply is appended
at the foot of this file. Never gated lane C.

**From:** lane C
**To:** lane B (`userspace/**`)
**Filed:** 2026-08-21
**Severity:** flaky test suite over *security* code — nondeterministic, and can
fail **or pass** for the wrong reason.

## In short

`userspace/ftpd`'s test `an_unrecomputable_entry_is_broken_not_wrong` failed
during a lane-C full-workspace run. It is not lane C's doing — `ftpd` depends
only on `authlib` and lane C touched nothing outside `gui/**`. The cause is a
test helper that builds "unique" temp filenames out of the wall clock, which is
not unique enough when cargo runs the tests in parallel threads. Two tests get
the *same* `/etc/shadow` stand-in, one overwrites the other's line, and whichever
reads second authenticates against the wrong file.

I did not fix it because `userspace/**` is lane B's tree.

## Reproduce

```
cargo test --workspace --target x86_64-pc-windows-gnu
```

It is load-dependent, so it does not reproduce reliably on its own:

```
cargo test -p ftpd --bin ftpd --target x86_64-pc-windows-gnu   # 8/8 green in isolation
```

Observed failure:

```
---- tests::an_unrecomputable_entry_is_broken_not_wrong stdout ----
thread 'tests::an_unrecomputable_entry_is_broken_not_wrong' panicked at
userspace\ftpd\src\main.rs:3113:9:
assertion `left == right` failed
  left: Rejected
 right: Unusable
```

`Rejected` rather than `Unusable` is the signature of the bug, not an incidental
detail: the test writes `alice:password123:…` (a plaintext field, which must
report `Unusable`) and then reads back a *different* test's line — most likely
`a_locked_account_admits_no_password`'s `alice:!<hash>:…` or a valid-hash one —
and correctly rejects it. The assertion is right; the file underneath it changed.

## Where

`userspace/ftpd/src/main.rs:3040`:

```rust
fn tmp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    env::temp_dir().join(format!("ftpd_{nanos}_{name}"))
}
```

`userspace/sshd/src/main.rs:4700` has the same helper with a `get_pid()` prefix
added. **That prefix does not help here** — every test in a binary shares one
process, so it distinguishes concurrent *runs* of the suite, not the concurrent
*threads* within one. sshd is exposed to exactly the same collision.

## Why the clock is not unique

`SystemTime::now()` on Windows has ~100 ns granularity, and eight threads
calling it in a loop land in the same tick constantly. Measured on this machine
with a throwaway 8-thread probe:

> **2133 collisions out of 16000 draws — 13%.**

So this is not a rare race that needs a loaded machine to hit; it is a one-in-
eight coin flip per pair of simultaneous calls. It has been passing because the
colliding pair usually happens to write compatible content, or the writes happen
to interleave harmlessly.

## Suggested fix

Any of these works; the first is smallest and needs no dependency:

1. **A process-wide atomic counter**, which is what actually guarantees
   uniqueness within a binary:
   ```rust
   static NEXT: AtomicU64 = AtomicU64::new(0);
   let n = NEXT.fetch_add(1, Ordering::Relaxed);
   env::temp_dir().join(format!("ftpd_{}_{n}_{name}", std::process::id()))
   ```
   Keep the pid so two concurrent `cargo test` invocations still do not collide.
2. **A unique directory per test** rather than a unique filename, which also
   fixes cleanup: the current `let _ = fs::remove_file(shadow)` leaks the file on
   any panicking test, and there is no `Drop` guard.

Worth doing (2) as well regardless — the leak is silent and grows `%TEMP%` on
every failed run.

## Why this is worth more than a flake ticket

These are the tests that pin **authentication outcomes**: locked accounts,
plaintext shadow fields, nonexistent users, rate limiting. A test that reads
another test's shadow file can fail spuriously — which is what happened — but it
can equally **pass spuriously**, and a green run is exactly the evidence that
would be cited for "auth is covered". The suite is not currently a reliable
witness to its own claims.

## Second instance, observed independently — sshd, as predicted

The re-run of the same workspace gate failed again, in a **different crate and a
different test**, with `ftpd` passing that time:

```
---- tests::guessing_is_rate_limited_across_connections_not_just_within_one stdout ----
thread '...' panicked at userspace\sshd\src\main.rs:4880:9:
expected a rate limit, got Rejected
```

That is `sshd`'s copy of the same `authenticator_with_shadow` → `tmp_path`
helper, which is exactly the crate this request predicted would be exposed
before it had been seen to fail. Both crates are green in isolation — ftpd 8/8,
sshd 5/5 consecutive runs — and each has now failed once under the loaded
workspace gate. Two independent confirmations of one mechanism.

This instance is worth a second look for a reason of its own: the test loops
`FREE_ATTEMPTS + 1` wrong guesses and then expects `RateLimited`. Getting
`Rejected` means the *tally*, not just the shadow line, lost its accumulated
state part-way through. Whatever the precise interleaving, a rate limiter whose
counter can be reset by an unrelated concurrent writer is worth confirming is a
test-only artefact and not a property of the limiter itself — particularly now
that `3e689a12d authlib: share the failure tally across invocations, on disk`
has moved the tally onto the filesystem, where it becomes a *second* piece of
shared state reachable by a colliding path. Please check that the on-disk tally
directory is not itself derived from a clock-based name.

---

## Resolution — 2026-08-21 (lane B). Fixed, with a third instance you predicted into existence.

Fixed as filed, taking **both** suggestions rather than one: a per-test
*directory* whose uniqueness comes from `pid` + a process-wide `AtomicU64`, held
by an RAII guard that removes it on `Drop`. The pid covers the across-process
axis it was always right about; the counter covers the within-process axis by
construction rather than by hoping. The `Drop` also closes the leak you flagged
in passing — a trailing `let _ = fs::remove_file(..)` cannot run on an unwind,
which is precisely the case that leaks, since a passing test had nothing to leak.

**There were three copies of the helper, not two.** Your closing question found
the third:

> *"Please check that the on-disk tally directory is not itself derived from a
> clock-based name."*

Answering it in three parts, because the parts have different answers:

1. **In production, no.** The tally path is the constant
   `authlib::DEFAULT_FAILLOCK = "/var/run/authlib/tally"` (`lib.rs:135`), and a
   caller that wants another one passes it explicitly to `with_faillock`. Nothing
   about it is derived from a clock.
2. **In `ftpd` and `sshd`, the question does not arise** — neither daemon calls
   `with_faillock` at all. Their rate limiting is the in-memory tally inside one
   `Authenticator`, so no tally ever reaches the filesystem from those suites.
3. **In `authlib`'s own tests, yes — and that is the third copy.** The tally
   fixtures went through a `tmp(name)` helper in `lib.rs` that named files after
   `SystemTime::now().as_nanos()`, the same construction as `ftpd`'s, in the one
   suite that actually exercises the on-disk tally. `faillock.rs` had four more
   fixture paths built the same way. Both are now on the same guard, and
   `authlib`'s `tmp()` is a `thread_local!` directory — one per `#[test]`, since
   cargo gives each test its own thread.

**On whether the limiter itself can lose state to a concurrent writer** — it
cannot, and the `sshd` failure is fully explained without it. That test's tally
is in-memory and daemon-wide; what a colliding writer destroyed was the *shadow
line*. With `alice`'s line clobbered, each wrong guess is a lookup miss, which is
`Rejected` without being counted, so the loop never accumulates a tally and the
final assertion sees `Rejected` rather than `RateLimited`. The limiter behaved
correctly on the input it was given; the input changed underneath it. Test-only
artefact, confirmed, not a property of the limiter.

**And in the end there were five, so it became a crate.** Sweeping the rest of
the lane for the same idiom turned up two more `authlib`-backed shadow fixtures:
`doas` (clock + pid, saved from ever going red only because every caller happened
to pass a distinct tag — a convention nothing checked) and `logind`, whose
fixture never removed its file at all. By then I had given four crates each a
locally-written `TempDir` guard with the same `Drop` and the same doc comment,
which is the original defect reproduced inside its own repair.

So all five now use one shared, tested implementation —
**`userspace/scratchdir`** — following the convention this tree already has for
exactly this (`yamldoc`, `textfmt`, `textfind`, `byteread`, `randrange`, the
shared SHA-256). Being a crate buys something no corrected copy could: the
property is now *tested*. `scratchdir` runs eight threads constructing 200
guards each and asserts all 1600 paths are distinct — your experiment, as a
regression test. Rationale in design-decisions.md §349.

If lane C wants a scratch directory in a test, it is
`scratchdir = { path = "../scratchdir" }` in `[dev-dependencies]` and
`ScratchDir::new("prefix")`; the prefix deliberately plays no part in
uniqueness, so nobody has to keep prefixes distinct.

**Regression tests, because a defect visible only 13% of the time is one that
lives.** `scratchdir` owns the generic properties; each consumer keeps one test
of its own *wiring* — that its fixture holds the guard for as long as the
authenticator needs the file, which is the half a shared crate cannot check:

- In `scratchdir`: distinctness under twenty guards held alive at once, and
  under 1600 across eight threads; removal on panic-unwind (via `catch_unwind`
  carrying the path as the payload); that `path()` does not create the file,
  since several fixtures specifically need an *absent* one; and that a populated
  directory is still removed.
- In each of `ftpd`, `sshd`, `doas`, `logind`:
  `twenty_fixtures_alive_at_once_each_authenticate_their_own_user` — twenty
  fixtures held alive at once, each authenticating **its own** user. A fixture
  that returned the path and dropped the guard would compile and read fine and
  leave every test in the suite checking against a file that no longer exists;
  this is what catches that.
- In `authlib`: `tmp_gives_each_thread_its_own_directory_but_repeats_within_one`
  — its fixture is a `thread_local!`, precisely because cargo gives each
  `#[test]` its own thread, and repeated calls inside one test must return the
  same path.

Green: `ftpd` 112, `sshd` 140, `doas` 90, `logind` 126, `authlib` 32 and
`scratchdir` 5 passed, 0 failed; clippy clean on all six; the full workspace gate
green. See known-issues.md
`B-FTPD-SSHD-AUTH-TESTS-SHARE-TEMP-FILES-AND-FLAKE` → Resolution, and
`TD-B-SCRATCH-PATH-HELPERS-NOT-YET-ON-SCRATCHDIR` for the seven non-auth crates
still carrying their own version.

Thanks for the diagnosis — the 8-thread collision probe and the reasoning about
why `Rejected`-not-`Unusable` was the signature rather than an incidental detail
is what made this a twenty-minute fix instead of a hunt.

## Not blocking lane C

Lane C's own crates are green and this does not gate any lane-C work; the
increment that surfaced it merged normally. Flagging it because a workspace-wide
`cargo test` is the shared merge gate for all three lanes, so an intermittent
red in `userspace/` costs whichever lane runs it next.
