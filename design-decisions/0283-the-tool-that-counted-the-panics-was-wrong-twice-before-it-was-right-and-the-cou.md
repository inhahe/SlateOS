## §283 — The tool that counted the panics was wrong twice before it was right, and the count moving is not evidence that it got better

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** To remove every "crash the machine if this fails" site (§282) we
first had to *find* them, and only the ones in real code — the same pattern in a
test is correct and there are ~1900 of those. The script that sorted real code
from test code got the sorting wrong twice, in opposite directions, and both
times its own comments confidently explained why it was fine. This records what
the bug actually was and the rule that came out of it.

### The two failures

Deciding "is this line inside a test?" means finding the function that encloses
it. Both wrong versions found *a* function; neither found the right one.

- **v1 tracked brace depth.** Braces appear in strings, in comments, and in
  closures, so the depth drifted. It filed every site in `oci.rs::self_test` as
  top-level code.
- **v2 used the nearest preceding `fn`.** These self-tests routinely define
  helpers inside themselves:

  ```rust
  pub fn self_test() {
      fn case(...) { ... }        // <- nearest preceding fn
      foo().expect("register");   // <- but this is still test code
  }
  ```

  `case` is not named `*test*`, so everything after it was reported as
  production. **25 sites across five files**, all false. v2's docstring
  explicitly called this failure mode "safe."

### The rule that fixes it

What matters is not the nearest enclosing `fn` but whether **any** enclosing
`fn` is a test. v3 keeps a *stack* of open functions and reports a site as test
code if any frame is. Indentation is the nesting proxy — sound here because the
tree is rustfmt-formatted, so a body is indented strictly further than its `fn`
line and the closing `}` sits at exactly that line's indent.

One subtlety, found by it breaking: pop with `<=` only when the line starts with
`}`, otherwise pop with strict `<`. Without the distinction, a multi-line
signature's `) -> Foo {` — at the same indent as its `fn` — pops the function
that was just pushed.

### The decision that matters more than the algorithm

**A count from an unvalidated classifier is not a measurement.** The number went
96 → 96 → 71 across three versions. It would have been easy to read the drop as
progress; in fact v1 and v2 were wrong in ways that happened to nearly cancel.
Every move was therefore justified by *reading the source at the disputed
sites*, not by the count changing.

The same discipline applied to the fix: after v3 cut 25 sites, a separate probe
looked for the opposite error — sites now hidden because some enclosing function
merely *looks* like a test. It found 14, under names that break the `*_test*`
convention (`self_test_inner`, `cmd_selftest`, `with_self_test_freed_address`,
and ten `self_test_*` in `syscall/linux.rs`). All were read; all are genuinely
self-tests. A fix that is not checked for over-correction is half a fix.

### Why a heuristic at all

The exact answer needs a parser — `syn`, or rustc's own. Rejected: this runs on
a `no_std` kernel tree from a plain Python script with no build step, and the
heuristic is checkable in the way that matters, because its output is a list of
file:line pairs that can be read. An exact tool that nobody runs is worth less
than an approximate one whose every disagreement was resolved by hand.
