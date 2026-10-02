### BUG-OILS-NAMEREF-CIRCULAR-CHAIN. A nameref cycle expands to the last name in the chain instead of to nothing, with no warning — 2026-07-28 — ✅ RESOLVED 2026-07-28

**Symptom.** A nameref chain that closes on itself:

```sh
declare -n a=b; declare -n b=a; echo "[$a]"
# bash: warning: a: circular name reference   →  []
# osh:                                        →  [b]
```

Longer cycles (`a→b→c→a`) behave the same way, and so does the in-function
self-reference `f() { local -n r=r; }`, where bash warns a *second* time at each
expansion (the declaration-time warning is already implemented — see
`Shell::nameref_value_error`).

**Where.** `userspace/oils/src/interp.rs` — `Shell::resolve_ref_name`. It walks
the chain with a 64-step bound and, on running out, returns whatever name it
reached, so the cycle silently resolves to a real (usually unset) variable name
rather than being reported.

**✅ RESOLVED 2026-07-28.** `resolve_ref_name` now carries a visited set and
returns `Option<String>` — `None` meaning "this chain names nothing" — so every
one of its ~20 call sites had to say what a cycle means for that operation.
Reads treat it as unset, writes fail, and `${!ref}` (which has no final name to
report) raises bash's `invalid indirect expansion` instead of a warning.

The diagnostic turned out **not** to need `&mut self`: `emit_stderr` is already
`&self`, so the warning lives in a thin wrapper, `resolve_ref_use`, which every
*use* of a name goes through while the handful of paths that must stay silent
(`${!ref}`, the readonly pre-checks that would double-report, `nameref_elem_value`
which resolves again immediately) call `resolve_ref_name` directly. The warning
names the variable the walk *started* from, matching bash for `a→b→c→a` read
from any link.

Fixing this exposed a second, unrelated divergence in the same area: `var_is_set`
(`-v`) never resolved namerefs at all, so `declare -n r=missing; [ -v r ]` said
*set* — it was testing whether the reference existed rather than its target.
It now resolves first; a target naming an array *element* (`declare -n e=arr[1]`)
is looked up as a plain variable name and so is never `-v` set, which is bash's
behaviour too.

**Coverage.** `tests/corpus/nameref.sh` grew two sections (unset/element targets
and the cycle), and `interp.rs` gained `nameref_cycle_reads_as_unset`,
`nameref_cycle_write_fails_and_leaves_the_cycle_intact` and
`nameref_to_unset_or_element_target_is_not_v_set`.

**Remaining cosmetic gap — see TD-OILS-NAMEREF-WARNING-COUNT below**, itself
closed on 2026-08-04.
