### TD-OILS-EXPORT-ARRAY-FLAG. `export -a` is accepted by bash and rejected by osh — 2026-07-30 — ✅ RESOLVED 2026-07-30 (listing half split out as TD-OILS-DECL-LIST-KIND-FILTER, also resolved)

**Where:** `userspace/oils/src/interp.rs` — `builtin_export`'s option parsing.

**What.** bash 5.2 accepts `export -a name` (an exported array is not actually
placed in the environment, but `declare -p` records the kind), while osh
reported an invalid-option usage error. `readonly -a` is accepted by both. The
asymmetry was an oversight in osh's `export` option table rather than a
deliberate choice.

**Fixed 2026-07-30.** `builtin_export`'s flag loop now takes `A`/`a` and applies
`array_kind_apply` under the same rule `builtin_readonly` uses — *only* on an
operand that carries a value, because the array is created by the assignment, so
`export -a fresh` leaves the name the plain scalar it was and `x=5; export -A x`
does not widen it. The call sits inside the valued branch, before the store, so
the value lands in the new array rather than beside it, and the append form then
reads element 0 back through `scalar_store` (`export -a v=1; export -a v+=2`
→ `([0]="12")`, matching bash). bash's usage synopsis still reads `[-fn]` and
omits the array flags even though it accepts them, so osh's matching usage line
is unchanged — deliberately, and noted in the code. Covered by
`export_takes_the_array_kind_flags_readonly_does`, which also pins that a
genuinely unknown letter still fails with status 2.

Re-measured against bash 5.2.37 rather than trusting the original note: the
observation above that `declare -p` shows `declare -ax name` after a *bare*
`export -a name` is wrong — bash shows `declare -x name`. The valued form is
what carries the kind.

**Prerequisite done 2026-07-30 — and it uncovered a worse bug.** Before `-a`
could apply an array kind, `export`'s assignment had to *use* one:
`builtin_export` wrote `self.put_var(k, stored)`, i.e. the scalar slot, so
`declare -a arr=(1 2); export arr=9` parked the 9 in a slot no expansion of an
array name ever reads — `declare -p arr` still showed `([0]="1" [1]="2")` and
the assignment was **silently lost**. bash gives `([0]="9" [1]="2")`. The append
form read the same dead slot, so `export arr+=9` appended to `""` instead of to
element 0.

Both halves now go through the array-aware pair `scalar_store` /
`set_scalar_store`, the same routing a bare `name=value` and `readonly
name=value` already used, which is also what makes `-a` implementable
(`array_kind_apply` would otherwise leave an empty array beside a live scalar).
Covered by `export_assigns_through_the_array_aware_store`.
