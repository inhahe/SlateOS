### TD-OILS-TRANSFORM-SUBSCRIPT-TWICE. `${a[$(cmd)]@a}` under `set -u` evaluates its subscript twice — 2026-08-01 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::param_transform`, the
nounset pre-check in front of the `@a` / `@A` branch, and
`Shell::transform_assign`, which resolves the element again.

**What.** bash faults on `${x@a}` and `${x@A}` for an unset parameter under
`set -u`, a variable declared without a value (`declare -i d`) included. Unlike
every other transform, those two answer from the *variable* rather than from
its value, so they never fetch it — the check therefore has to fetch it itself,
and fetching evaluates the subscript. `transform_assign` then evaluates the
subscript a second time to render the element:

```
$ bash -uc 'declare -A m; m[k]=v; echo "${m[$(echo k >&2; echo k)]@A}"'
k            # subscript evaluated once
```

osh writes `k` twice. It happens only with `set -u` on, only for `@a`/`@A`, and
only when the subscript has a side effect.

**Fixed in `76f2f1364`,** by threading the element through as the report above
proposed — `transform_assign` now takes the element rather than the subscript
word, which is what makes the one read reusable. The nameref obstacle the report
named turned out to be narrower than it looked: `param_elem_value` resolves the
nameref itself, so the check's read and the render's read *are* the same value
whenever the check read the parameter at all. The one case they differ is an
**indirection**, whose operand the caller already resolved to the *name* a
nameref holds — reusing that would make `${!r@A}` recreate the target with its
own name for a value — so reuse is conditional on `Operand::Param`.

**The rule was wider than the report knew.** Measuring the whole matrix (both
operators × nounset on/off × element present/absent) showed osh wrong in *four*
of the combinations, not one, and bash's contract to be:

| first read | count |
|---|---|
| finds an element | 1 |
| finds nothing | 2 — bash asks the whole reference again, for either operator |
| finds nothing, `set -u` on | 1 — the fault falls between the two asks |

The second ask is a full one, so `${n[-9]@a}` reports `bad array subscript`
*twice*. No other operator does this: `@Q` on the same missing element reads
once. osh's two other errors were reading twice for `@A` under `set -u` (the
reported bug) and reading *zero* times for `@a` with nounset off, because the
pre-check was guarded by `unbound_is_error` precisely to avoid the double read.

Covered by the corpus case `a-a-variable-transform-reads-its-element-once.sh`
and the lib test
`a_variable_transform_asks_again_only_when_the_first_ask_found_nothing`.

Two neighbouring divergences turned up while measuring this and are logged
separately: TD-OILS-TRANSFORM-SCALAR-IGNORES-SUBSCRIPT and
TD-OILS-WHOLE-ARRAY-TRANSFORM-IS-ONE-WORD.
