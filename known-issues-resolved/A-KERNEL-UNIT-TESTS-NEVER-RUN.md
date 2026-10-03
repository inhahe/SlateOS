## `A-KERNEL-UNIT-TESTS-NEVER-RUN` — found 2026-08-22 (lane A) — FIXED 2026-08-22

**In short:** The kernel contains 54 blocks of code marked as automated tests,
spread over 8 files. None of them has ever run. They are not merely skipped —
they are never compiled either, so the compiler has never checked them and they
are free to have rotted into code that would not build. Anyone reading those
files sees tests and reasonably concludes the code beneath them is checked. It
is not. The worst case is the file that handles filenames, which has ten of
these and no other test of any kind.

### How this happened, and why the cause is not itself a mistake

`kernel/Cargo.toml` sets `test = false` on the kernel binary, with a correct
rationale: the kernel is `#![no_std]` and supplies its own `panic_impl` lang
item, which conflicts with host `std`, so a host-side test harness genuinely
cannot link it. Kernel code is tested by *boot self-tests* under the bare-metal
target instead — `self_test()` functions called from `main.rs`, which is the
mechanism that has produced essentially all of this tree's real kernel coverage.

That decision is sound and is not what is being reported here. The defect is that
54 `#[test]` functions were written *anyway*, against a harness that was never
going to run them. There is no `lib.rs`, so `[[bin]] test = false` removes the
only target a test harness could attach to:

```
$ cargo test -p kernel --target x86_64-pc-windows-gnu
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.87s
```

No `Compiling kernel`, no `running N tests`, no test binary. Nothing built,
nothing ran.

### Where they are

| File | dead `#[test]`s | had a `self_test()`? | outcome |
|---|---|---|---|
| `kernel/src/fs/ext4/vfs_impl.rs` | 13 | yes | deleted; 4 gaps ported |
| `kernel/src/fs/pathutil.rs` | 10 | **no** | **converted** |
| `kernel/src/net/frag.rs` | 7 | yes (hermetic only) | **converted** |
| `kernel/src/net/httpd.rs` | 7 | yes | deleted; 3 gaps ported |
| `kernel/src/fs/ext4/driver.rs` | 6 | yes | deleted; 1 gap ported |
| `kernel/src/tty/mod.rs` | 6 | yes | deleted; fully redundant |
| `kernel/src/fs/ext4/balloc.rs` | 3 | yes | deleted; 1 gap ported |
| `kernel/src/net/raw.rs` | 2 | **no** | **converted** |

Six of the eight had a boot self-test, so those modules were not wholly
unchecked — but in four of the six the existing self-test turned out to be a
strict superset of the dead tests, and the two that had *no* other coverage
(`pathutil.rs`, `raw.rs`) were the ones that mattered. `pathutil.rs` was the
priority: path handling is a trust boundary (`CLAUDE.md` self-review items 7 and
8 — bytes not UTF-8, resolve before crossing), it is reached by every `open`,
and its ten tests had never once executed.

### Why "never compiled" is worse than "never run"

A skipped test is a test you can run. Dead `#[cfg(test)]` code is not
type-checked at all, so it drifts with every refactor of the code it tests and
nothing objects. By the time someone enables it, the failures will be a mix of
real regressions and tests that simply no longer match the API — and telling
those apart costs far more than writing them again. Assume some of the 54 do not
currently compile; that is the expected state, not a surprise.

### The fix

Convert them to boot self-tests, which is the mechanism that actually runs. This
also fixes lane B's `raw.rs` report for free: boot self-tests run sequentially on
one CPU, so the `CLAIMED`/`OWNER` interleaving they traced is impossible by
construction rather than by a mutex.

The conversion is not mechanical, and three constraints keep recurring:

- **A boot self-test cannot `assert!`.** A failed assertion is a panic and a
  panic during boot is a dead machine, not a failed test. Every check has to log
  its specific failure and return `Err`.
- **The kernel state is real.** Unit tests could assume an empty process table
  or an idle global; a boot self-test cannot. `raw.rs`'s two hardcoded PID 4242
  and relied on it being absent — now searched for and skipped if unavailable.
  Anything writing a live global has to restore it on every exit path.
- **Re-read each test against the current API.** They have never been
  type-checked, so treat compilation as an open question per file.

### Resolution (54 of 54)

| Date | Files | Result |
|---|---|---|
| 2026-08-22 | `pathutil.rs` (10), `raw.rs` (2) | converted; boot PASS 1674 s, both print `Self-test PASSED` |
| 2026-08-22 | `frag.rs` (7) | converted; found `A-FRAG-REJECTED-FRAGMENTS-STILL-CLAIM-A-REASSEMBLY-SLOT` |
| 2026-08-22 | `tty/mod.rs` (6) | **deleted** — every case already covered by the live self-test |
| 2026-08-22 | `ext4/balloc.rs` (3) | deleted; ported 1 gap (`find_free` starting *on* the free bit) |
| 2026-08-22 | `ext4/driver.rs` (6) | deleted; ported 1 gap (block far beyond the requested range) |
| 2026-08-22 | `httpd.rs` (7) | deleted; ported 3 gaps (non-numeric status line, rate-limit table init, 429 content type) |
| 2026-08-22 | `ext4/vfs_impl.rs` (13) | deleted; ported 4 gaps (error *variant* on both split failures, SOCK/UNKNOWN and BLK/FIFO fallbacks, `..` file_type byte) |

`grep -rn '^#\[cfg(test)\]' kernel/src/` now returns nothing. The only remaining
`#[test]` occurrences in the crate are inside doc comments that explain why the
convention is boot self-tests.

**19 of the 54 were worth keeping and 35 were not** — which is the useful
finding, not the count. The five files whose dead tests were deleted were not
under-tested; their live `self_test()` had simply outgrown the unit tests years
earlier and nobody had removed the corpse. Two of those files
(`httpd.rs`, `vfs_impl.rs`) had drifted so far they would no longer *compile*:
both called functions with `&str` long after the signatures moved to byte-based
`Path`/`&[u8]` so that non-UTF-8 filenames survive. That is the "never compiled
is worse than never run" prediction above landing exactly as described.

The nine ported gaps were all small and all real — the pattern in them is that a
live self-test tends to check the cases someone was debugging at the time, and
skip the boundary next to them: the free bit you are *standing on* rather than
scanning toward, the block far past the window rather than one past it, the
status line that is long enough but not numeric, the second of two nearly
identical writers.

The `frag.rs` conversion is the argument for doing the rest. Its seven dead
tests all drove the module-level `add_fragment`, i.e. the global reassembly
table — a layer the module's existing `self_test()` deliberately avoids in order
to stay hermetic. Porting them therefore meant reading a code path nothing
covered, and a remotely-triggerable table-exhaustion DoS was sitting in it. The
value here is not only "the tests run now"; it is that porting a test forces
someone to read the path it covers.

`tty/mod.rs` is the counter-example, and worth knowing about before starting the
next file. Its `self_test()` already carried a doc comment saying the unit tests
below "do not run on the bare-metal custom target, so this mirrors their
assertions" — the problem was known *there*, and handled, long before this issue
was filed. All six properties really are checked by the running self-test, which
also covers a good deal more (erase/kill/EOF, all three ISIG signals and the
line flush each performs, `NOISIG`, echo rendering, chunked delivery, pgrp
ownership). Verified case by case rather than taken on the comment's word; the
six were deleted and the comment corrected.

So: **check what the existing `self_test()` already covers before porting.**
Duplicated coverage is not free — it is more code to keep correct, and it makes
the real gaps harder to see. The two useful outcomes per file are "ported,
because it covered something nothing else did" and "deleted, because it did
not"; the one to avoid is "ported without checking".

Two things to get right while converting:

- **A boot self-test cannot `assert!`.** A failed assertion in the kernel is a
  panic, and a panic during boot is a dead machine rather than a failed test.
  The established pattern is `Result<(), &'static str>` with the caller logging
  and continuing, so a broken test reports and the boot survives.
- **Don't convert blindly.** Each one has to be re-read against the current API
  before it is trusted, per the point above. A converted test that passes because
  it no longer tests anything is worse than the dead one, which at least does not
  claim to pass.

### How this was found

Lane B's `scripts/raced-globals.py` flagged `raw.rs`'s `CLAIMED`/`OWNER` as
raced by two unserialised `#[test]`s, and filed
`requests/b-a-raw-nic-claim-tests-race-and-the-reader-is-the-writer.md`. The
analysis was correct about the interleaving and could not have been correct about
the consequence, because the tests do not run. Checking that before applying the
suggested mutex is what turned up the larger problem. Reply in
`requests/a-b-raced-globals-flags-tests-that-cannot-run.md`, which also suggests
the checker skip crates whose test target is disabled.
