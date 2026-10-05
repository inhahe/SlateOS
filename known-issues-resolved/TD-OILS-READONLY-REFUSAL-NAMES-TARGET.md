### TD-OILS-READONLY-REFUSAL-NAMES-TARGET. A refused write through a nameref names the resolved target where bash names the reference — 2026-07-28 — ✅ RESOLVED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::apply_assignment_spelled`,
`Shell::arith_write_dest` / `Shell::arith_blame`, `Shell::arith_elem_writable`.

**What.** A nameref redirects a store to its target, and everything the store
*does* belongs to the target — but a readonly refusal does not always say so.
It names the **reference** for an *array-shaped* write and the **target** for a
scalar one. That is the same seam as everywhere else a nameref meets an array
(compare the `+a` refusal above, and `Shell::resolve_ref_array_write`): a write
that makes an array is about the reference's own name.

The original entry called this "the target is array- or assoc-valued", which is
wrong on both halves; a 60-row probe of both shells gave the real rule.

**The rule, measured.** For a plain assignment the write is array-shaped when
the *operand as written* has a subscript or the value is a compound literal:

| written                       | target            | blamed |
|-------------------------------|-------------------|--------|
| `r=5`, `r+=5`                 | readonly scalar   | target |
| `r=5`                         | readonly array    | target |
| `r[0]=5`, `r[k]=5`            | anything readonly | **ref** |
| `r=(a b)`                     | anything readonly | **ref** |
| `r=5`, ref is to `q[1]`       | readonly array    | target |
| `declare -i r; r=5`           | readonly array    | target |

For an *arithmetic* write the rule is the same plus one case: a bare operand
whose target is an **indexed** array is really `q[0]=…`, so it too is
array-shaped. An associative one is not — measured, not reasoned:

| written                        | target             | blamed |
|--------------------------------|--------------------|--------|
| `((r=5))`, `((r++))`, `let r=5`| readonly scalar    | target |
| `((r=5))`, `((r+=2))`, `((++r))`| readonly indexed array | **ref** |
| `((r=5))`                      | readonly assoc     | target |
| `((r[0]=5))`, `((r[k]=5))`     | anything readonly  | **ref** |
| `((r=5))`, ref is to `q[1]`    | readonly array     | target |

Along a chain the reference blamed is the **outermost** name — the one the
reader can see — not the link it went through. Every arithmetic entry point
(`((`, `let`, `$(( ))`, `for ((;;))`) spells it the same way. Without a nameref
the two names are one, so nothing is observable there.

**Fixed** by threading the assignment *as written* through the nameref rewrite
(`Shell::apply_assignment_spelled`, which also fixed the `set -x` trace — see
below) and by giving the arithmetic write paths a blame name of their own
(`Shell::arith_blame`). Corpus:
`tests/corpus/an-assignment-through-a-nameref-is-traced-and-blamed-by-the-name-as-written.sh`
and
`tests/corpus/an-array-shaped-arithmetic-write-through-a-nameref-is-refused-by-the-name-as-written.sh`.

**The trace, fixed with it.** The same probe turned up a second divergence
nobody had logged: `set -x` shows an assignment **as typed**, never as
resolved. `declare -n r=x; r=5` traces `+ r=5`, and a reference that designates
an element traces `+ r=9` rather than the `x[1]=9` the store became. osh traced
the resolved form.

**Already matched before this.** A *temporary environment* assignment is its
own path and names the reference whatever the target is
(`Shell::prefix_assignments`, covered by `tests/corpus/env-prefix.sh`):

```
$ bash -c 'declare -n n=r; readonly r=1; n=2 true'   # n: readonly variable
$ bash -c 'declare -n n=r; readonly r=1; n=2'        # r: readonly variable
```

**Found by the same probe, fixed straight after:** bash *expands before it
checks*, so a refused write still runs the RHS's side effects and still traces
— see TD-OILS-A-REFUSED-ASSIGNMENT-STILL-EXPANDS-AND-TRACES.
