## §286 — A guard that must be invoked to work is a guard that can be silently dropped, so the two found this way were moved to where nothing has to remember them

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Two safety checks in this tree looked like they were running and
were not. One said in its own documentation "call once at boot" and no boot
path called it. The other said its exit status was 1 when it found a problem,
so a script could stop the build on it — but it always returned 0, and no
script called it either. Neither was broken code; both would have worked
perfectly if anything had used them. The fix in each case was to move the check
somewhere it happens *because it exists* rather than because someone remembered
to call it: one is now a compile-time assertion that fails the build, the other
is now wired into the boot test with a working exit status.

### How they were found

Not by looking for them. The kernel's clippy backlog — about 18 000 warnings —
was being split into production and test code to find out how much of it
mattered, since a `.unwrap()` in a boot-time self-test is a test asserting on
its own fixture and a `.unwrap()` in a syscall path is a denial of service.
The split came back with **zero** production `unwrap`/`expect` sites and
exactly **three** production `panic!` sites, which is small enough to
adjudicate one at a time rather than in bulk. Two of the three were correct and
already argued for in their own doc comments. The third was
`mm::kvspace::validate`, and reading it to decide whether its panic was
justified is what surfaced that nothing called it.

That is worth stating on its own: **the value of getting the count down to
something adjudicable is that you then read each survivor, and reading them
finds things the count never would.** The panic in `validate` was never going
to fire. The bug was next to it.

### The two, and what changed

**`mm/kvspace.rs` — a runtime check over compile-time constants.** `validate()`
walked `ALL_REGIONS` looking for overlapping kernel VA ranges and panicked on
one. Every region in that list is a `const`, so the answer never depended on
anything that happens at runtime; the function was asking a compile-time
question at boot, and only when the self-test ran. It is now:

    const _: () = assert!(first_overlap().is_none(), "…");

`first_overlap` is a `const fn`, so the search happens during compilation and a
kernel whose regions overlap does not link. The rejected alternative was to
keep `validate()` and call it from `kernel_main`, which would have made the
check real but left it dependent on that call surviving every future
reshuffling of the boot sequence — the same fragility that produced the bug.

Two smaller calls inside this one. The `panic!` message could not name the
offending regions, because a const-evaluated `panic!` takes a literal and does
no formatting; `first_overlap` returns the *indices* rather than a `bool` so a
caller that wants the names can still have them, and in practice the pair is
whichever region was just added. And the module's self-test no longer asserts
`first_overlap().is_none()` — that is now a tautology, true of any kernel that
exists to run it. It tests the `overlaps` predicate instead, against cases with
known answers on both boundaries: regions that merely abut must not overlap, a
one-byte overlap at either end must. A gate whose comparison is wrong passes
everything, and would certify exactly the layouts it was written to reject.

**`scripts/scan-unwrap.py` — a gate with no caller and no exit status.** Its
`main()` ended in `return 0`. Any script written the documented way — `if
scan-unwrap.py; then` — would have passed on a tree full of findings. It now
returns 1 on findings and runs from `boot-test.sh` as `check_production_unwrap`,
beside the user-access gate from §285. Verified the way §285 established: by
injecting a production `.unwrap()` and confirming the gate fails, not by
watching it pass on a clean tree.

### The generalisation, which is the actual finding

Both are instances of **a guard whose documentation asserts an enforcement that
nothing performs**, and that makes three in short succession — §285's
`write_user_image` had a `# Safety` contract asserting what its caller happened
to arrange. In none of the three was the code wrong. What was missing was the
wiring, and wiring is invisible in the file you are reading: `validate()` reads
like a boot check, `scan-unwrap.py` reads like a gate, `write_user_image` reads
like it has preconditions someone enforces.

Two questions find this class cheaply, and they are worth asking of anything
that presents itself as a check: **does anything call this, and does anyone
look at what it returns?** A search for callers answers the first in seconds.
The second is the one that caught `scan-unwrap.py`, and it is the easier one to
skip, because a script that prints its findings *looks* like it is working.

### Consolidating the classifier, and why it was safe to

Splitting the clippy backlog needed the *outermost* enclosing function, not the
nearest one above the line. This kernel is a `no_std` binary, so `cargo test`
never runs and its tests are ordinary `pub fn self_test()` functions that
routinely wrap each case in a nested `fn case()`. A nearest-preceding
classifier blames `case`, which is not named `*test*`, and files thousands of
test sites under production — it inflated the production bucket by ~665 sites
before it was caught at `fs/progmgr.rs:929`.

`scripts/scan-unwrap.py` had already solved this (§283) using indentation as
the nesting proxy. Rather than leave a second implementation beside it — which
is the mistake §285 was written about — the logic moved to
`scripts/rust_scopes.py` and both scripts now use it. It tracks brace depth
through a lexer that understands strings, raw strings, char literals and
comments, which is what §283 rejected brace-counting for: "raw strings, char
literals and macro bodies all perturb the count". True of a naive counter, so
this one is not naive, and it is checked rather than asserted — it closes every
scope it opens across all 800 files of `kernel/src`, and one desynchronisation
anywhere would leave a dangling scope at some file's EOF.

The switch was not taken on the strength of that argument alone. Both
classifiers were run against all 4 407 panicking sites in the tree and compared
site by site: **zero disagreements**. That makes the consolidation a measured
no-op rather than a hoped-for one, which is the only basis on which replacing a
working gate's internals is worth doing.

The indentation proxy was not wrong. But it worked because the tree is
rustfmt-formatted — a precondition the script asserted in its docstring and
nothing checked, which is the same shape as everything else in this entry.

### Addendum — the gate had a hole shaped like a naming convention

Wiring `scan-unwrap.py` into the build made its exemption rule load-bearing for
the first time, which is a good moment to read it. A site is exempt if any
enclosing function has `test` in its name. `kshell::eval_test` is the shell's
`test` / `[` builtin: it parses a user-supplied expression, runs on every
`[ -f "$x" ]` a script executes, and was silently exempt from a gate whose
whole purpose is to stop a user-shaped input from panicking the kernel.

Nothing was hiding there — it has no `unwrap` sites, and the tree-wide scan
found no site exempted under any suspicious name. That is the reason to fix it
now rather than later: the hole is currently empty, so closing it is a
three-line change with no findings to adjudicate, and the gate that has been
reporting zero keeps meaning what it says.

The fix is a `NOT_TESTS` set of production functions whose names collide with
the convention, each with its reason inline — the same shape as
`ALLOWED_HHDM_WRITERS` in §285, and for the same reason: an exception that
lives in a named list with a comment stays visible to the next reader, whereas
one expressed as a cleverer regex does not.

A call graph would answer the real question — is this reachable from anything
but the boot suite — and the name never will: nothing about `eval_test` and
`tx_datapath_test` distinguishes a shell builtin from a driver self-test.
That was considered and not built. The convention holds nearly everywhere, the
exceptions fit on one screen, and a list that is wrong is wrong visibly, which
a call graph that is wrong is not. Verified by injecting an `.unwrap()` into
`eval_test` and confirming the gate now fails on it.

### Addendum 2 — the predicate test failed on its first boot, which is the point of having written it

The `self_test` above was written to pin `overlaps` against cases with known
answers rather than re-assert a compile-time truth, on the argument that a gate
with a broken comparison passes everything. Boot 295 panicked on it:

    panicked at kernel\src\mm\kvspace.rs:346:5:
    an empty region overlaps nothing

`overlaps` was the textbook two-comparison form, `a.start < b.end() &&
b.start < a.end()`. That is correct for non-empty intervals and wrong for empty
ones: a zero-size region has `start == end`, so `start < end` is false in both
directions and *both* comparisons pass — it is reported as overlapping whatever
contains it. The assertion was right and the predicate was wrong.

This is worth recording for two reasons beyond the fix.

**The degenerate case was the one that mattered.** The five interesting
assertions — abutting, one-byte overlap at either end, identical, contained —
all passed first time. The predicate was correct on everything a person thinks
to check and wrong on the case that gets written down only out of
completeness. A test suite that covers the interesting cases and skips the
degenerate one would have certified this.

**A zero-size region is a real bug, and it now has its own message.** The first
instinct was to make `overlaps` return false for empty regions and stop there,
which is mathematically right but would have let a typo'd `size:` constant
through the build entirely — the overlap gate is the only thing looking, and it
would now deliberately ignore it. So `first_empty` is a second `const`
assertion with its own text. Reporting a zero-size region as an "overlap" would
send the reader hunting for a second region that is not the problem; reporting
it as nothing at all is worse. Verified by setting `FAULT_TEST.size = 0` and
confirming the build fails with the size message rather than the overlap one.

The general form: when a predicate is corrected so that it deliberately stops
reporting a class of input, check what *was* catching that class. Usually the
answer is "only this", and the correction has quietly deleted a check.
