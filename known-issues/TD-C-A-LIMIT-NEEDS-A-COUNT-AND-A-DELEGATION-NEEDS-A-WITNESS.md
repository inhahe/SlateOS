## `TD-C-A-LIMIT-NEEDS-A-COUNT-AND-A-DELEGATION-NEEDS-A-WITNESS` (lane C, 2026-09-21)

**Status:** OPEN (a practice, recorded from three instances in two lanes)

**In short:** a checker can be honest about what it does not cover and still
mislead, in two different ways. It can *state* a limit without saying how big
it is, so nobody can weigh it. Or it can hand the uncovered part to another
tool — and nobody ever runs that tool. Both read as diligence.

**The three instances, one day, two lanes.**

| the claim | what was true |
|---|---|
| `frozen-flag-survey`: "construction is not a write; some types are data" | three exclusion rules, each a sentence, together setting aside **569 fields against 888 scanned** — more than it judged |
| lane A's `scan-guest-output`: "reads only unprefixed lines" | the eligible population was **109 lines of 47,626**, 0.23% |
| lane A's `check-doc-links`: "the other three classes do not need a gate — rustdoc reports them the moment anyone runs `cargo doc`" | **every mention of `cargo doc` in the repository is inside that sentence.** Nobody had ever run it. First run: 94 seconds, 430 warnings in the kernel |

**The two cures are different and neither substitutes for the other.**

*A limit needs a count.* A reader cannot tell thirty excluded fields from
three thousand, and the prose reads identically either way. The fix is to
print the size of what was set aside next to the answer -- which is what
`frozen-flag-survey` does now, and which immediately showed its set-aside
population to be the larger one.

*A delegation needs a witness.* "Tool X covers the rest" is a claim about
tool X's existence and use, and **no test on either side of the handoff can
see it** -- the delegating tool's suite proves the delegating tool works, and
the delegate has no suite because nobody runs it. The only check is to go and
run the thing the docstring names. Lane A's five-month gap is the cost of not
asking.

**And a third relative, from the same exchange: a proxy inside the guard.**
Lane A's `--roots` floor refuses a verdict when a scan sees fewer than five
*crates*, reporting "1 crate(s), 807 file(s), 6,078 link(s)" in the same
breath. The floor is right -- an empty scan must not read as clean -- but it
counts the wrong noun for a tree that is one crate. Same shape as this lane's
scope regex matching `impl App for X` when every app writes
`impl oswindow::app::App for X`: the proxy and the thing came apart, and
nothing said so.

**What this lane does about it now.** Every checker docstring gets read for
the phrase "X covers the rest", and X gets run. `frozen-flag-survey`'s one
such handoff is the `..Default::default()` and destructuring-assignment case:
nothing catches those, so it is a limit with no count and no delegate, which
by the rule above is the weakest kind. It is not fixed; it has stopped being
described as covered.
