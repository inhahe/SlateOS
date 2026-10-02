### TD-OILS-ARRAY-EMPTY-ASSIGNED. `declare -p` / `@A` can't distinguish a never-assigned empty array from an assigned-empty one — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `format_var_assignment` (~8792) and
the array/assoc state (`Shell::arrays`, `Shell::assoc`). Surfaces in `declare -p`,
the bare `set` listing, and the whole-array `${arr[@]@A}` transform.

**Symptom:** bash distinguishes an array that was *declared but never assigned*
from one that was *assigned an empty value list*, and prints them differently:

```
declare -a e;    declare -p e   →  bash: declare -a e       osh: declare -a e     (match)
declare -a e=(); declare -p e   →  bash: declare -a e=()    osh: declare -a e     (DIFF)
```

The same split applies to `${e[@]@A}` (bash `declare -a e=()` vs osh
`declare -a e`) and the `set` listing. osh currently prints an empty array/assoc
as the bare name in *both* cases. (A non-empty array is unambiguous and already
matches; the divergence is only for the empty-but-assigned state.)

**Why deferred:** osh's model stores every array as a `HashMap`/`BTreeMap` entry;
both `declare -a e` (`arrays.entry(name).or_default()`) and `e=()`
(`arrays.insert(name, empty)`) produce an identical empty container, so the two
states are indistinguishable without extra bookkeeping. A proper fix needs an
"has been assigned a value list (even empty)" marker per array — set on every
value-assignment site (`name=(…)`, `name[i]=…`, `+=`, `e=()`) but **not** on a
bare `declare -a`/`declare -A` — cloned into subshells and cleared on `unset`.
That touches many mutation sites for an obscure cosmetic distinction, so it was
split out. (An earlier attempt to unconditionally emit `=()` for empty arrays
was reverted: it fixed the `e=()` case but regressed the far more common
`declare -a e` no-init case.)

**Proper fix:** add `Shell::arrays_assigned: HashSet<String>` (covering indexed +
associative), set it at each value-assignment site, honour it in
`format_var_assignment` (empty + assigned → `name=()`, empty + not-assigned →
bare `name`), clone it in `clone_for_subshell`, and remove on `unset`.

**RESOLVED (2026-07-19):** implemented as `Shell::array_valued: HashSet<String>`,
mirroring the `integer_attr` lifecycle. Set at every value-assignment site
(the `AssignRhs::Array` indexed + assoc literals — including empty `a=()`; the
indexed element-assign and `a+=` element-0 paths; `assoc_set`; `read -a`;
`mapfile`/`readarray`), **not** on a bare `declare -a`/`-A`. Cloned into
subshells, snapshot/restored by `local` (added `array_valued` to `VarSnapshot`),
and removed on `unset` (whole variable). `format_var_assignment` now emits
`name=()` for an empty-but-valued array/assoc and the bare `name` otherwise.
Fixed the related `read -a arr < /dev/null` bug in the same change: `read -a`
now resets the target array to empty up front (bash semantics), so an EOF with
no data leaves a defined empty array and a pre-existing array is replaced rather
than merged. Tests: `declare_p_empty_array_distinguishes_assigned_from_declared`,
`read_a_creates_empty_array_on_eof`. Verified against MSYS bash across
element-assign-then-unset, `+=`, `local -a`, `mapfile`, and pre-existing-array
cases. 548 tests pass; clippy + slateos builds clean.
