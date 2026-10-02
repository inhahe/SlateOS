## TD-C-A-TEST-NESTED-IN-ANOTHER-FUNCTION-COMPILES-AND-NEVER-RUNS -- METHOD 2026-09-17

**In short:** a `#[test]` written inside another function's body compiles
cleanly, is never collected by the test harness, and reports nothing. The suite
stays green and the test simply does not exist. It took three attempts to
notice, because every signal looked like success.

**Date:** 2026-09-17. **Lane:** C.

**What happened.** A patch appended a test to
`gui/desktop/src/session/tests.rs` and landed it *inside* the last function in
the file, four spaces deep. Rust allows nested items, so it built. Running it
by name gave:

    test result: ok. 0 passed; 0 failed; 2842 filtered out

`ok` with zero passed. The count before the test was added was also 2842.

**Why it survived two fixes.** The file is included with `#[cfg(test)] mod
tests;`, so its top-level items sit at **column 0** -- there is no `mod tests {
... }` wrapper. Two attempts to "move it to module level" inserted it before
the file's final `}`, which closes the last *function*, not a module. Both
attempts reported success and changed nothing, because the block moved from
inside one function to inside the same one.

**The signals, and which one was honest:**

| Signal | What it said |
|---|---|
| `cargo build` / `clippy` | clean -- a nested item is legal |
| `cargo test <name>` | `ok. 0 passed` |
| **The total count** | **2842 before, 2842 after** |

Only the third was informative, and only because the number happened to be in
front of me from an earlier run. `ok. 0 passed` is the same shape as the
filtered-run trap already recorded in
`TD-C-THREE-TESTS-AND-TWO-CHECKS-THAT-PROVED-NOTHING-IN-ONE-DAY`: **a green
result about nothing.**

**The habit that catches it in one step:** after adding a test, check that the
suite's total went up by the number of tests added. Not that it passed -- that
it *exists*. A filtered run cannot tell "passed" from "absent"; the count can.

**And before moving code in an unfamiliar file, read the indentation of its
top-level items.** Four spaces meant "inside something" here, and the file's
own structure -- a `mod` declared elsewhere -- is invisible from inside it.
