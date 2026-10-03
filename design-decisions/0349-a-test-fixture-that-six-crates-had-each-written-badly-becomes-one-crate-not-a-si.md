## §349 — A test fixture that six crates had each written badly becomes one crate, not a sixth correction

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)

**In short:** Six programs' test suites each contained a hand-written helper
that made a scratch file in the system temp directory. Every copy named the
file after the current time, and every copy was wrong for the same reason: the
clock is not a unique identifier, so two tests running at once could get the
same file and each end up checking its answers against the other one's data.
Five of the six were tests of *password checking*, so that could make a broken
login look correct just as easily as it could make a working one look broken.
The decision was to delete all six and put one shared, tested implementation in
a small crate (`userspace/scratchdir`), rather than fix each copy in place.

### What was actually wrong

Two independent defects, both present in every copy.

**The clock is not a unique id — at any resolution.** This is the part that
looks fixable and is not. The instinct on seeing a collision is to add digits,
or reach for a finer clock, or swap `SystemTime` for `Instant`. None of that
helps, because the system clock is *refreshed on a timer interrupt* rather than
recomputed per read: two threads reading it inside one tick read the same value
however many digits that value has. And `cargo test` runs a binary's tests as
threads of one process, so "two threads inside one tick" is the ordinary case
rather than a rare race. Lane C measured it with an eight-thread probe on this
machine: **2133 collisions in 16000 draws — 13%**.

Mixing in `std::process::id()`, which three of the six did, does not fix it
either, though it reads as though it should. The pid separates concurrent
*runs* of a suite — a second `cargo test` in another window — which is a real
axis and worth keeping. It says nothing whatever about the threads inside one
run, which is the axis that collides.

**The cleanup line cannot run in the case that leaks.** Each copy ended its
tests with `let _ = fs::remove_file(path)`. That runs only if the test reaches
the end of its body — that is, only if the test *passed*, which is precisely
the case with nothing interesting left behind. A failing assertion unwinds
straight past it. So the tests that leaked were exactly the ones nobody cleaned
up after.

### Why a crate rather than six corrections

The obvious move after lane C's report was to fix the two named crates. That
would have left the defect in four others, including — found only by following
lane C's closing question — `authlib` itself, the crate both reported failures
were *about*.

More to the point, the fix had already been copied before it was finished. Four
crates had, by then, each received a locally-written `TempDir` guard: the same
struct, the same `Drop`, the same doc comment restating the same reasoning four
times. That is the shape of the original defect reproduced in the repair, and
this tree has a settled answer for it — `yamldoc`, `textfmt`, `textfind`,
`byteread`, `randrange` and the shared SHA-256 all exist because some helper had
been pasted into a dozen files and gone subtly wrong in each. `scratchdir` is
the same move, with the same justification, applied to a test fixture.

Making it a crate also buys something a corrected copy cannot: the property can
be *tested*. `scratchdir` runs eight threads constructing 200 guards each and
asserts all 1600 paths are distinct — the exact experiment that fails 13% of the
time against the code it replaces. No single consumer would have written that.

### What the consumers keep

The split is deliberate, and it is what stops this becoming a shared crate that
everyone still wraps in a local helper:

| Property | Tested where |
|---|---|
| Two guards never share a directory, including under concurrency | `scratchdir` |
| The directory survives to the end and is removed on panic-unwind | `scratchdir` |
| `path()` does not create the file (fixtures need *absent* paths) | `scratchdir` |
| *This* crate's fixture holds its guard as long as the thing using the file | each consumer |

That last row is the one a shared crate genuinely cannot check. A fixture that
returns the `PathBuf` and drops the guard compiles, reads correctly, and leaves
every test in the suite authenticating against a file that no longer exists. So
each of the five auth crates keeps exactly one test — twenty fixtures alive at
once, each authenticating *its own* user — and `authlib` keeps one for the
thread-local wiring that gives each `#[test]` its own directory.

### The alternative considered and rejected

Vendoring `tempfile` (the ecosystem's answer) was the other option. Rejected on
the same grounds as the other shared crates here: it is a dependency with a
dependency tree, for something whose entire correct implementation is a counter,
a pid, and a `Drop`. The reasoning that makes it correct is worth writing down
in this tree, where the next person to need a scratch directory will look —
which is the actual failure mode, since all six authors of the broken helper
were writing in a tree that already contained five other copies of it.

### What is not yet converted

`oils`, `crond2`, `du`, `firejail`, `wc`, `tail` and `filekind` have their own
scratch-path helpers, of the same two shapes: clock-derived (`oils`, six sites)
and fixed-name (`crond2_test_anacron`, `du_test_walk`, `firejail_test_parse` —
which cannot collide between tests but *do* collide between two concurrent runs
of the suite). None of them tests authentication, which is why the auth five
went first. Tracked in known-issues.md as
`TD-B-SCRATCH-PATH-HELPERS-NOT-YET-ON-SCRATCHDIR`.
