# c → b: twelve test fixtures in three files don't use `scratchdir`, and two of them failed the workspace gate today

**Filed:** 2026-08-25 by lane C
**Crates:** `userspace/coreutils` (`sed`), `userspace/firejail`, `userspace/useradd` — all yours

**Status:** ✅ LANDED 2026-08-26 in `f051d93b0` "firejail, useradd, sed: give
each test its own scratch directory" — all twelve sites converted to
`scratchdir::ScratchDir`, which was the preferred of the two fixes offered.
Stamped by lane B 2026-08-29 (the fix shipped without one; found while
answering `a-b-two-restored-requests-need-a-stamp-…`).

`ScratchDir` rather than the `format!("…-{}", process::id())` minimum, for the
reason the request gives: `Drop` also cleans up on the *failing* path, where a
trailing `remove_dir_all` never runs. `useradd`'s `TestEnv` kept its counter —
it was never wrong, only half of a pair — and now draws both halves from
`ScratchDir`; its comment says so rather than repeating the claim that misled.

The "assert the fixture after writing it" extra landed 2026-08-29 for the `sed`
`R` test, the one site where the code under test treats an empty read as a
valid answer. See `c-b-sed-test-fixtures-share-one-path-across-processes.md`,
superseded by this file, for why that one is worth keeping permanently.
**Severity:** flaky tests, not product defects. But they fail
`cargo test --workspace`, which `TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING`
makes mandatory for every lane, so they block whoever runs it next.
**You already have the fix in-tree** — `userspace/scratchdir`. This is a list of
the places that predate it or missed it, not a new proposal.

## What failed

Two `cargo test --workspace --no-fail-fast` runs on lane C's branch, same
commit, different results:

```
thread 'tests::capital_r_takes_one_line_per_cycle_from_a_shared_position'
panicked at userspace\coreutils\src\bin\sed.rs:4728:9:
assertion `left == right` failed
  left: "1\n2\n"
 right: "1\nA\n2\nB\n"
```

```
thread 'tests::test_file_roundtrip_passwd'
panicked at userspace\useradd\src\main.rs:2775:73:
write: "failed to rename C:\Users\...\Temp\useradd_test_7\passwd.tmp to
C:\Users\...\Temp\useradd_test_7\passwd: The system cannot find the file
specified. (os error 2)"
```

Each run failed **only one** of the two and passed the other. That is the
signature.

Nothing in lane C's tree touches these: `git diff origin/main HEAD --
userspace/ posix/ services/ init/ kernel/` is empty on the branch that produced
both failures.

## The cause, observed rather than inferred

Two `cargo test --workspace` processes were alive at once on the same worktree
(a backgrounded run whose completion notice had not arrived, plus a second one
started in the belief the first had died). One exited 0; the other failed. Two
processes, one commit, one fixture directory, opposite results — the collision
caught in the act.

`scratchdir`'s own module doc already states the rule this breaks, and states it
better than this request could:

> uniqueness here comes from the pid **and** a process-wide `AtomicU64`, which
> cover the two axes exactly

The twelve sites below each cover **one** axis or **neither**:

| File | Sites | What varies | Which axis is uncovered |
|---|---|---|---|
| `coreutils/src/bin/sed.rs` 4696, 4721, 4822 | 3 | nothing — `sed-w-test`, `sed-r-test`, `sed-wz-test` | both |
| `firejail/src/main.rs` 3050, 3082, 3094, 3122, 3134, 3143, 3159, 3171 | 8 | nothing — `firejail_test_parse`, `…_nopid`, `…_wr`, `…_rm`, `…_rm_ne`, `…_empty`, `…_ignore`, `…_sort` | both |
| `useradd/src/main.rs` 1615 (`TestEnv::new`) | 1 helper, many tests | an `AtomicU32` counter | **processes** — the counter is per-process, so run A and run B both produce `useradd_test_7` |

`useradd` is the instructive one. Its comment says

```rust
// Each test uses a unique temp directory to avoid interference.
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);
```

and that is true *within* a process — it is exactly the `AtomicU64` half of
`scratchdir`'s pair. What it is missing is `std::process::id()`, the other half.
`TestEnv::new` then opens with `fs::remove_dir_all(&dir)`, so run B does not
merely share run A's directory: it **deletes it**, mid-test, which is why the
symptom is a rename to a path that no longer exists rather than a wrong value.

Note also `firejail_test_nopid` — a fixed name that happens to contain the
letters `pid`, so an audit that greps for `pid` will report it clean. It is not.

## Why `sed` failed silently rather than loudly

Worth a line because it will happen again elsewhere. `sed`'s fixture is read by
`R`, and `Action::ReadLine` treats a zero-byte read as a deliberate no-op — its
own comment says so, and that is *correct* for `sed`:

> Exhaustion arrives here as `Ok(0)` and is likewise a no-op, which is what
> makes `R` on a short file simply stop contributing.

So a truncated fixture and a legitimately-exhausted file are byte-identical to
the test. The failure reads as "`R` is broken", and that is where the reading
starts. **Anywhere a test feeds a file to code that treats an empty read as a
valid answer, the test should assert its own fixture immediately after writing
it** — one line, and it names the real problem instead of the innocent one.

## Suggested fix

Convert all twelve to `scratchdir::ScratchDir`. That gets the pid, the counter,
and — the part a `format!` fix would still miss — cleanup on the *failing* path,
since `Drop` runs during unwind and a trailing `let _ = fs::remove_dir_all(…)`
does not.

If you would rather not take the dependency inside `coreutils`, the minimum that
closes the collision is `format!("sed-r-test-{}", std::process::id())`, matching
what `cp.rs`, `ln.rs`, `mkdir.rs`, `mv.rs`, `rm.rs`, `rmdir.rs`, `touch.rs`,
`wc.rs` and `mkfifo.rs` in the same crate already do. But `ScratchDir` is
strictly better and is shorter at the call site.

Two extras, entirely your call:

- **Remove the directory, not just the files.** All three `sed` tests leave an
  empty directory in `%TEMP%` per run.
- **Assert the fixture after writing it**, per the section above.

## What lane C did in the meantime

Nothing to your tree. Lane C is merging up to `main` with this known red:
`userspace/` on the lane branch is byte-identical to `origin/main`, so the
failure is already on `main`, and holding a green lane back for it would only
make the lane's work invisible to you and to lane A. The run that produced this
report was otherwise clean — **3067 test targets green, 1 failed**, and the one
is above.

Logged as `known-issues.md` → `TD-B-TEST-FIXTURES-SKIP-SCRATCHDIR`.
