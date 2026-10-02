### TD-OILS-ARITHMETIC-NEVER-ASKS-WHETHER-A-SUBSCRIPT-NAMES-NOWHERE. `(( a[-7] ))` and `(( a[-7]=1 ))` are silent where bash refuses both — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, the `VarLookup for Shell` impl —
`get_index_str` (the arithmetic read) and `set_index` (the arithmetic store).
Neither asked `subscript_is_bad`, which the *expansion* path has always asked
(`${a[-7]}` was already right). `set_index` also open-coded a fragment of
`assign_elem`'s indexed branch, with the wrong bound and without
`array_kind_apply`, which cost three further divergences of its own.

**Reproduce.** `userspace/oils/tests/corpus/a-bad-arithmetic-subscript-is-named-by-the-read-and-spelled-by-the-store.sh`
(50 checks). The core of it:

```sh
a=(1 2 3); (( a[-7] ))           # bash: a: bad array subscript          osh: silent
a=(1 2 3); (( a[-7] = 1 ))       # bash: a[-7]: bad array subscript      osh: silent
c=9;       (( c[-1] ))           # bash: c: bad array subscript, → 0     osh: → 9
c=9;       (( c[-1] = 4 ))       # bash: declare -a c=([0]="4")          osh: declare -a c
c=9;       (( c[1]  = 4 ))       # bash: ([0]="9" [1]="4")               osh: ([1]="4")
unset u;   (( u[-1] = 4 ))       # bash: refused, u stays unset          osh: declare -a u
```

**What bash does.** A negative subscript counts back from one past the highest
index the name *holds*; when that underflows, the subscript names nowhere. The
read and the store are two different pieces of code and disagree about almost
everything except the question:

| | read (`expr_streval` → `array_reference`) | store (`expr_bind_variable`) |
|---|---|---|
| spelling | `a: bad array subscript` — name only | `a[-7]: bad array subscript` — name **and** subscript |
| which name | the one the reference **resolved** to (`declare -n r=a` → `a`) | the one **written** (`declare -n r=a` → `r`) |
| the subscript | not quoted | the **source text, verbatim**: `a[-3-4]`, `a[ -7 ]`, `a[-q]` |
| the bound | highest index + 1; a scalar holds **0** | same, except a scalar holds **1** |
| on failure | value 0, expression carries on | store dropped, expression keeps its value |

So `+=` earns both lines in that order, a plain `=` only the second, and
`c=9; (( c[-1] ))` is refused while `c=9; (( c[-1] = 4 ))` succeeds and replaces
the 9 — the asymmetry `Shell::elem_write_bound` already documented for the
expansion path.

The store's *verbatim* spelling is the surprise, because it survives a read that
had already evaluated the subscript (`(( a[-q] += 1 ))` still says `a[-q]`). It
is the same `lind == -1` branch: a read whose subscript named nowhere never
reaches `curlval.ind`, so `expassign` sends the whole `a[sub]` text down
`expr_bind_variable` rather than the `vname[ind]` `expr_bind_array_element`
would have rewritten it to (expr.c:365-388, 600-604).

**What else the measurement turned up.**

* A refused store stops where it stands. It never converts a scalar
  (`c=9; (( c[-2] = 4 ))` leaves `declare -- c="9"`), never creates an unset
  name, and never pays for the walk it did not take: through a circular chain
  `(( c1[-1] = 5 ))` warns **twice** and leaves `c1` a reference, where
  `(( c1[3] = 5 ))` beside it warns three times and replaces it with an array.
* The subscript is judged **before** the reference is. `declare -n r='q[0]';
  (( r[-1] = 9 ))` is `r[-1]: bad array subscript`, not the
  ``((: `q[0]': not a valid identifier`` the same store with a *good* subscript
  earns.
* Both complaints are untagged by the builtin (no `((:`/`let:` prefix), unlike
  an arithmetic error, and both arm `set -e` without failing on their own.
* Only an *indexed* array asks: `declare -A m; (( m[-7] = 2 ))` stores under the
  key `-7`. And nothing is asked under suppression, where no read and no store
  happens at all.

**The fix.** Both sites now ask the question the expansion path asks, through
the pieces that were already factored out:

* `get_index_str` resolves the name once, takes the bound from
  `highest_index_bound` (0 for anything that is not an indexed array — which
  covers a reference landing on an element or closing on itself, blamed as
  written), complains and reads 0.
* `set_index` takes the raw subscript text as a new parameter, resolves the
  target *silently* with `resolve_ref_name`, judges the subscript against
  `elem_write_bound`, and only then commits — `arith_elem_write_commit`, split
  out of `arith_elem_write_base`, is where the walk's warnings and the unmaking
  of a circular reference now live, so a refused store never pays for them. The
  commit then calls `array_kind_apply(base, false)`, which is what carries a
  scalar into element 0, and drops the `entry().or_default()` that used to
  create an array for a store that never happened.
* `arith::Ix` became a struct keeping the subscript's source text beside the
  index it resolved to, because a read-modify-write still spells the source.
