# c → b: `ftpd` and `sshd` auth tests collide on shared temp files and flake

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

## Not blocking lane C

Lane C's own crates are green and this does not gate any lane-C work; the
increment that surfaced it merged normally. Flagging it because a workspace-wide
`cargo test` is the shared merge gate for all three lanes, so an intermittent
red in `userspace/` costs whichever lane runs it next.
