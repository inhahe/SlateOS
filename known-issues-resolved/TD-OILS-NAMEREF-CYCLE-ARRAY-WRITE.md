### TD-OILS-NAMEREF-CYCLE-ARRAY-WRITE. A write through a circular nameref refuses in `osh` where bash warns and writes the raw name — 2026-08-03 — ✅ **FIXED 2026-08-05** (arithmetic warning counts remain; see below)

**Where:** `userspace/oils/src/interp.rs` — `Shell::resolve_ref_array_write` /
`resolve_ref_elem_write` / `resolve_ref_write_walks` / `is_circular_ref`, and
their callers `whole_array_write_target`, `apply_assignment_inner`,
`set_scalar_target_checked` and `exec_for`.

**What.** bash's cycle detection does not abandon every write. Which of its two
answers you get is chosen by the **shape of the value**, and probing turned that
into a rule the tracker's original sketch did not have:

* A **scalar** store has nowhere to go: one warning, status 1, every variable in
  the cycle untouched — and a bare `c1=x`, being a failed variable assignment,
  ends the shell.
* An **array** store does not fail. `nameref_transform_name` warns and hands
  back the name the walk *started* from, so the write lands on the reference
  variable itself, which stops being a reference (bash has no array namerefs:
  `declare -a a; declare -n a=x` is `reference variable cannot be an array`).

That second half is the load-bearing one, and it is more than cosmetic: osh's
refusal turned `c1=(a b)`, `c1[0]=v` and `c1+=(a)` into *failed assignments*, so
where bash carried on the shell **exited**.

Three details only measurement gives:

* The warning is printed **once per walk of the chain**, so a subscripted target
  — which bash resolves twice, once to find the array and once to bind the
  element — reports it twice where a whole-array fill reports it once.
* The old value goes with the attribute. `c1[3]=v` through a cycle is
  `declare -a c1=([3]="v")`, not `([0]="c2" [3]="v")`: a reference's value is
  the *name* it pointed at, and keeping it would make it element 0 of the array
  the store is creating.
* Dropping the attribute is what makes the shell **usable again** rather than
  merely quiet. `c2` still names `c1`, and with `c1` no longer a reference the
  cycle is gone — so `${c2[0]}` reads what was just written.

A `for`/`select` control variable is a case of its own: bash does *not* follow a
nameref there (`declare -n r=t; for r in v` leaves `t` alone and makes `r` a
reference to `v`), yet a circular chain still stops it — one warning, status 1,
and not one iteration, discovered on the first iteration the way the readonly
refusal beside it is, so an empty list stays silent. Hence `is_circular_ref`,
which asks the question without the resolution.

**Fixed 2026-08-05.** Pinned by
`a-circular-nameref-write-lands-on-the-name-it-started-from.sh` (byte-exact
against bash 5.2.37, warning counts included) and the lib test
`a_circular_nameref_write_lands_on_the_name_it_started_from`.

**Arithmetic — ✅ also fixed 2026-08-05.** The arithmetic forms used to land
their value correctly but under-report the warning, because osh resolved the
name once where bash walks the chain again for each of its internal steps — and
the element forms were worse than that: `(( c1[0] = 5 ))` warned not at all,
left the nameref attribute on (`declare -an c1=([0]="5")`), and a *read* of
`c1[0]` fell back to reading the reference's own value as element 0, so the
recursive arithmetic evaluation of that value produced a warning blaming the
**next** name in the cycle (`c2`) rather than the one written.

The count model that fits every measurement is **one warning per walk of the
chain**, with each shape walking it a fixed number of times:

| shape | walks | example |
|---|---|---|
| scalar read | 1 | `(( c1 ))`, `$(( -c1 ))`, `let "c1"` |
| element read | 2 | `$(( c1[0] ))` — once to find the array, once to fetch |
| scalar write | 2 | `(( c1 = 5 ))`, `let "c1 = 5"` |
| element write | 3 | `(( c1[0] = 5 ))`, `let "c1[0] = 5"` |

and the counts simply add, so a read-modify-write pays for both halves:
`(( c1 += 5 ))` and `(( c1++ ))` warn three times, `(( c1[0] += 5 ))`,
`(( c1[0]++ ))` and `(( ++c1[0] ))` five.

The two writes also differ in *kind*, not just in count. A **scalar** write
stores nothing and leaves the name a reference — `declare -p c1` after
`(( c1 = 5 ))` still says `declare -n c1="c2"` — because no array is being made
for the attribute to be incompatible with. An **element** write lands on the
name the walk started from and drops the attribute, exactly as `c1[0]=v` does,
so the cycle is broken and `${c2[0]}` reads what was written.

**Fixed by** giving the read side the same walk-count parameter the write side
already had: `Shell::resolve_ref_use_walks(name, n)` (with `resolve_ref_use` now
`n = 1`), `Shell::arith_elem_read` (2 walks) beside the silent `arith_elem` that
the *parser* uses for `is_assoc`, and `Shell::arith_elem_write_base` (3 walks,
via the existing `resolve_ref_write_walks`) replacing the duplicated
array-or-refuse dance in `set_index`/`set_assoc`. `arith_write_dest` asks for 2
walks directly rather than going through `scalar_write_dest`'s 1. A new
`ArithElem::Circular` variant is what stops the read falling back to the
reference's own value — the compiler now forces every element lookup to say
what a cycle means to it.

**Coverage.** Corpus case
`arithmetic-through-a-circular-nameref-warns-once-per-walk.sh` (byte-exact
against bash 5.2.37, counts included) and the lib test
`arithmetic_through_a_circular_nameref_warns_once_per_walk`.

**Still open — two neighbours, both tracked elsewhere.**
`declare -i c1` on a circular operand draws two lines from bash and one from
osh; that is the declaration-builtin half of TD-OILS-NAMEREF-WARNING-COUNT
below, along with the parameter-expansion reads (`${c1[0]}`, `${c1[@]}`,
`${#c1}`, `unset 'c1[0]'`) that bash also doubles. Separately,
`$(( c1[@] ))` was a bash arithmetic *subscript* form osh's lexer rejected
outright — unrelated to namerefs, and not tracked before; it was found here and
fixed the same day as TD-OILS-ARITH-AT-SUBSCRIPT below. *(Both of these are
closed now: TD-OILS-NAMEREF-WARNING-COUNT was finished on 2026-08-04.)*
