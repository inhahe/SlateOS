### TD-OILS-A-SUBSCRIPTED-ARITHMETIC-OPERAND-DOES-NOT-FOLLOW-A-NAMEREF-CYCLES-ESCAPE. `(( arr[1] ))` through an escaped cycle read 0 and wrote nowhere — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs` (`ArithElem`, `Shell::arith_elem_read`,
`arith_elem_write_base`/`_commit`, `VarLookup::{note_arith_unbound, get_index_str,
is_assoc, get_assoc_str, set_index, set_assoc, refuse_whole_array_subscript}`) and
`userspace/oils/src/arith.rs` (the three trait signatures those needed).

A cycle that closes on a function-local reference resolves at *global* scope
(`find_variable_nameref`, variables.c:2074), and the unsubscripted operand
already followed that escape. The subscripted one did not: `ArithElem::Array`
carried a bare `String`, so every lookup behind it — the array's kind, its
negative-subscript bound, the element, the store — was made in the *current*
scope, where the local reference stands in front of the global. Reading a scalar
global that way read the reference's own value (`"z"`) and recursively
arithmetic-evaluated it, which is where the spurious third `z: circular name
reference` came from.

**The fix.** `ArithElem::Array { base, scope }` carries the `RefScope` the walk
named, and each of the four lookups goes through `Shell::in_scope`; the two
stores resolve to a `(String, RefScope)` and wrap their readonly guard and their
table work in one swap, as `Shell::in_dest_scope` does.

**And the walk counts, which are the other half of the measurement.** bash reads
a subscripted operand through *two* lookups and an unsubscripted one through
*one* (expr.c:1180):

```c
  v = (e == ']') ? array_variable_part (tok, tflag, …) : find_variable (tok);
  …
  value = (e == ']') ? get_array_value (tok, aflag, &es) : get_variable_value (v);
```

`array_variable_part` runs whatever `set -u` says, but osh's
`note_arith_unbound` early-returned when `!nounset` — so `arith_elem_read`
walked *twice* to compensate, which was right with `+u` and one too many with
`-u`. Now the check makes the first walk unconditionally for a subscripted
operand (and none of its own for an unsubscripted one, which shares bash's
single `find_variable` with the read), `arith_elem_read` makes one, and
`refuse_whole_array_subscript` takes the count as a parameter — 1 on the read
side, where the check already paid, and 2 on the store side, where nothing has.
On the write side the warnings moved out of the `target.is_none()` branch and
into `warn_circular_walks`, so a circular chain that *escaped* warns as often as
one that reached nothing — it just keeps its reference, having found somewhere
to put the value.

Every count below is measured against bash 5.2.37, not reasoned:

| form (through an escaped cycle) | walks |
|---|---|
| read `a[i]`, read `m[k]` | 2 |
| read `x` | 1 |
| write `a[i]` | 3 |
| write `m[k]` (no subscript to measure) | 2 |
| write `x` | 2 |
| `a[i] += 5` | 5 |
| `m[k] += 4` | 4 |
| `a[@]` read, `a[@]` store | 2 each |
| `a[]` | 0 |
| write `a[i]` the subscript refuses | 2, and the reference survives |

**Corpus:** `a-subscripted-arithmetic-operand-follows-a-nameref-cycles-escape.sh`.
Unit test: `a_subscripted_arithmetic_operand_follows_a_nameref_cycles_escape`,
beside `arithmetic_through_a_circular_nameref_warns_once_per_walk`, which pins
the non-escaping counts and is unchanged.
