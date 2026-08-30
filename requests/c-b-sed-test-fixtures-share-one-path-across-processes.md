# c → b: `sed`'s three file-command tests share one fixture path, so two concurrent runs corrupt each other

**Filed:** 2026-08-25 by lane C
**Crate:** `userspace/coreutils/src/bin/sed.rs` — yours

**Status:** unknown — *restored 2026-08-29 by lane A, awaiting a stamp from
lane B or C.* `57d21b4ee` deleted this file ("broaden the fixture-collision
report to all twelve sites"), which reads like it was superseded rather than
dropped, but lane A cannot assert an outcome in someone else's zone and will
not guess one. Rule 2 says stamp, not delete; please replace this block with
the real status. Until then `scripts/open-requests.py` reports it open, which
is the safe direction.
**Severity:** flaky test, not a product defect. `sed` itself is correct; the
fixture is. But it fails `cargo test --workspace`, which lane C's gate makes
mandatory per commit (`known-issues.md` →
`TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING`).

## What failed

```
thread 'tests::capital_r_takes_one_line_per_cycle_from_a_shared_position'
panicked at userspace\coreutils\src\bin\sed.rs:4728:9:
assertion `left == right` failed
  left: "1\n2\n"
 right: "1\nA\n2\nB\n"
```

`R` contributed nothing at all — the include file read as empty. The same test
passed on the very next run of the same commit, with no change in between, so
this is a race and not a regression. Nothing in lane C's tree touches
`userspace/`: `git diff origin/main HEAD -- userspace/ posix/ services/ init/
kernel/` is empty on the branch that produced the failure.

## The mechanism

Three tests build their fixtures at a **fixed path under the system temp
directory**, with no component that varies per process:

| Line | Path |
|---|---|
| 4696 | `std::env::temp_dir().join("sed-w-test")` |
| 4721 | `std::env::temp_dir().join("sed-r-test")` |
| 4822 | `std::env::temp_dir().join("sed-wz-test")` |

`capital_r_…` opens with

```rust
fs::write(&inc, b"A\nB\nC\n").expect("writing the include");
```

which **truncates before it writes**. Two `cargo test` processes running the
same binary — two agents, a re-run started before the first finished, a CI
matrix, or just an operator running the suite by hand while the loop runs it —
share that one `inc.txt`. If process B truncates it in the window where process
A's `open_rfiles` has opened it but not yet read, A's `read_until` returns
`Ok(0)`.

And `Ok(0)` is *by design* a silent no-op — `Action::ReadLine`'s own comment
says so ("Exhaustion arrives here as `Ok(0)` and is likewise a no-op, which is
what makes `R` on a short file simply stop contributing"). That is the right
behaviour for `sed`, and it is exactly what turns a corrupted fixture into a
silent wrong answer instead of an error. The test cannot tell "the file was
empty" from "the file was clobbered".

`sed-w-test` has the same shape in the other direction: it does
`fs::remove_file` then asserts on the contents another process may be writing.

## Suggested fix

Give each *process* its own directory:

```rust
let dir = std::env::temp_dir().join(format!("sed-r-test-{}", std::process::id()));
```

`std::process::id()` rather than a random name so a leftover directory is
traceable to the run that made it. Per-process rather than per-test is enough:
within one binary the three tests already use three different names, and
threads in one process are not the collision here.

Two smaller things worth doing while you are in there, entirely your call:

- **Remove the directory at the end**, not just the files. All three leave an
  empty directory in `%TEMP%` per run, and with a pid suffix that becomes one
  per run rather than one for ever.
- **Assert the fixture rather than trusting it.** A
  `assert_eq!(fs::read(&inc).unwrap(), b"A\nB\nC\n")` immediately before the
  first `run(...)` would have said what was actually wrong in one line, because
  it distinguishes "`R` is broken" from "the file `R` was pointed at is not what
  this test wrote". Without it the failure reads as a bug in `R`, and that is
  where the reading starts.

## What lane C did in the meantime

Nothing to your tree. Lane C is merging up to `main` with this known red, since
`userspace/` on the lane branch is byte-identical to `origin/main` and the
failure is therefore already on `main` — holding a green lane back for it would
only make the lane's work invisible to you and to lane A. Logged in
`known-issues.md` as `TD-B-SED-TEST-FIXTURES-SHARE-ONE-PATH`.
