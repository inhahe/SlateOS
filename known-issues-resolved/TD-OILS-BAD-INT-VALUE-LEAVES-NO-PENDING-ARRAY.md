### TD-OILS-BAD-INT-VALUE-LEAVES-NO-PENDING-ARRAY. A name left behind by a bad `-i` value did not become a *valued* empty array — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `Shell::note_int_refusal_assigned`,
`Shell::scalar_write_store`, and the two scalar `is_int` failure paths in
`Shell::apply_assignment_inner`.

**The bug.** A malformed `-i` value stores nothing, but bash still counts the
name it was aimed at as *assigned*: it clears `att_invisible` on the way to the
arithmetic, and the error jumps out past the store without putting it back.
Nothing shows while the name is a scalar — `declare -p` reports the same bare
declaration either way and `${n+SET}` stays empty — but the distinction survives
to be read once the name becomes an array. osh dropped it:

```sh
declare -i n=2+       # both: 2+: syntax error: operand expected
declare -p n          # both: declare -i n   (and ${n+SET} empty in both)
declare -a n; declare -p n
                      # bash: declare -ai n=()        osh: declare -ai n
```

**What was measured.** Every write aimed at the *whole variable* marks it,
whichever builtin asked — `declare -i n=2+`, `n=2+`, `n+=2+`, `export n=2+`,
`readonly n=2+`, `local -i n=2+`, `read n`, `printf -v n`, and a write through a
nameref — and the associative kind reads the flag the same way (`declare -A s`
after `declare -i s=2+` gives `declare -Ai s=()`). A write aimed at an *element*
does not (`declare -ai e; e[0]=2+` keeps the bare `declare -ai e`, and likewise
through `declare -n re='e[0]'` or `read 'e[0]'`), and neither does an array
literal, whose pairs never reach the table (`declare -Ai m; m=([k]=2+)` and its
`+=` form both stay bare). A whole-array write *does* mark it, because it emptied
the array before it failed (`read -a w`, `mapfile -t w` → `declare -ai w=()`), and
`declare -ai a=(1 2+)` keeps the prefix it managed to store. `unset` clears the
flag again.

**The fix.** osh spells bash's not-`att_invisible` as `Shell::array_valued`, which
only the array listings read, so a scalar that never becomes one is unaffected.
`Shell::note_int_refusal_assigned` records it, and is called from the three
places a whole-*variable* store can be refused for a bad `-i` expression: the
`None` arm of `scalar_write_store` (guarded on a `ScalarDest::Var` destination,
so an element write does not mark its array) and the plain and `+=` scalar
`is_int` arms of `apply_assignment_inner`. The element and array-literal paths
were already correct and were left alone.

Covered by the lib test `a_refused_integer_value_still_counts_as_an_assignment`
and the corpus case
`a-a-refused-integer-value-still-counts-as-an-assignment.sh`.
