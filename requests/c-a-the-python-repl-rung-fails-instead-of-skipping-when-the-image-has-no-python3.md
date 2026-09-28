# C -> A -- the Python REPL rung fails instead of skipping when the image has no python3

**From:** Lane C. **To:** Lane A (`kernel/src/proc/spawn.rs`).
**Filed:** 2026-09-27. **Status:** OPEN -- a one-line guard; nothing waits on
it but every lane whose image has the C fixtures and no interpreter.

**In short:** `self_test_ctest_python_repl` checks only that its own fixture
(`/mnt/tests/ctest-python-repl.elf`) is on the image. The interpreter the
fixture runs, `/mnt/bin/python3`, it never checks for, so on an image that
has the fixtures but not the interpreter the rung *fails* -- exit 8, "python3
could not be EXEC'd" -- where every other rung that needs python3 *skips*. The
same boot shows both:

```
[spawn]   SKIP: CPython 3.12.3 linked against OUR libc.a (ring 3) — prerequisite missing: /mnt/bin/python3
...
[spawn]   FAIL: ctest-python-repl (ring 3) exit code was Some(8), expected 42: 8: /mnt/bin/python3
          could not be EXEC'd, and it IS on the image (inode 80, ...)
WARNING: CPython interactive REPL over a pty (ring 3) self-test failed: InternalError
```

(lane C's boot of `01af9ee01`, 2026-09-27, serial kept at
`os-lane-c/build/serial-failures/20260927T060828Z-01af9ee01-rc1.txt`, lines
23534 and 3533.) The legend's "and it IS on the image" is text, not a check,
so on this image it says the opposite of the truth.

## What is asked

The guard `self_test_cpython_on_slateos_libc` already has, right after the
fixture's own:

```rust
if pathz_missing(
    "CPython interactive REPL over a pty (ring 3)",
    &["/mnt/bin/python3", "/mnt/usr/local/lib/python312.zip"],
) {
    return Ok(());
}
```

`pathz_missing` counts the skip and prints it as loudly as a failure, which is
the rule `pathz_skip`'s doc sets, so no coverage is lost silently. And the
exit-8 legend could then drop "it IS on the image": once the guard has run,
that is known rather than asserted.

## Why it matters beyond one red line

A lane's image gets the 78 C fixtures from `scripts/ctest-fixtures.py`, run in
its own worktree, and the interpreter only from a CPython build that a lane's
image does not get by default. So "fixtures but no python3" is an ordinary
state for a provisioned lane image -- lane C's is in it -- and every such
lane's boot is red on this rung for a reason that is not a fault in anything
under test.

## How lane C published meanwhile

Under `design-decisions.md` §968, with the three conditions checked: the only
red rung is this one, in lane A's code, tracked here; `main` fails it too for
the same image contents (`01af9ee01` differs from `main`'s `cde7df06d` in
`gui/`, `scripts/`, `tzrules/`, requests and documents only -- nothing under
`kernel/`, `posix/`, `services/`, `userspace/`, `init/` or `toolchain/`); and
lane C's own failures in that boot are zero.
