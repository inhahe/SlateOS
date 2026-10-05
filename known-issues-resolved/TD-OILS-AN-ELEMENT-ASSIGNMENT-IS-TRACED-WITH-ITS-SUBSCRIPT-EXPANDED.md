### TD-OILS-AN-ELEMENT-ASSIGNMENT-IS-TRACED-WITH-ITS-SUBSCRIPT-EXPANDED. bash traces the target as written and the value as expanded — FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, the `set -x` line emitted by
`Shell::apply_assignment_inner`, and `userspace/oils/src/unparse.rs`, which had
no way to render an assignment's left-hand side alone (`assignment_src` rendered
the whole thing, so a subscripted assignment could only be traced in source
form, value included).

**The rule.** `set -x` shows an assignment's two halves from two different
places. The target is the word bash **scanned** for a name; the value is the
string it **expanded** — because by the time `do_assignment_internal` traces,
only the value has been through expansion. The subscript has not: it is
evaluated later still, inside `assign_array_element`. So:

```sh
f() { echo "v$1"; }
i=2; set -x
a[$i]=$(f e)      # bash: + a[$i]=ve
a[1+1]=$(f a)     # bash: + a[1+1]=va
```

A subscript makes no difference to which of the two forms the trace takes — that
is decided by the value alone, and a compound literal is the one traced whole in
source form, since none of it has been expanded yet.

**The fix.** `unparse::assignment_src` was split so `assignment_target_src`
renders just the name and, when there is one, the subscript as written;
the trace line is that plus the operator plus the *expanded* value. The
`trace_scalar` predicate dropped its `spelled.index.is_none()` clause, which was
what previously pushed subscripted assignments onto the whole-source path.

**Corpus:** `an-element-assignment-is-traced-with-its-value-expanded.sh`.

**How it was found:** the corpus case for the expansion-order fix above traced
`tr3[0]=$(f tracedelem)` where bash traced `tr3[0]=vtracedelem`.
