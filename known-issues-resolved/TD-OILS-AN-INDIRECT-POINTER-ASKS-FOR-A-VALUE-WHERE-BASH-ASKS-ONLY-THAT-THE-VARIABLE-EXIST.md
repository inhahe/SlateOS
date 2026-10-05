### TD-OILS-AN-INDIRECT-POINTER-ASKS-FOR-A-VALUE-WHERE-BASH-ASKS-ONLY-THAT-THE-VARIABLE-EXIST. `declare -A s=([k]=v); ${!s:-x}` complains where bash yields `x` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::indirect_pointer_value` /
`Shell::resolve_ref_name` — the "invalid indirect expansion" test, which fires
whenever the pointer *expands to nothing*.

**Reproduce.**

```sh
b() { printf '%-10s ' "$1"; ( eval "printf '<%s>' $1"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
declare -A s=([k]=v)   # exists, but s[0] is unset
declare -a t; t[1]=v   # ditto
declare -a e0=()       # ditto
declare -a d0          # declared only — invisible
declare z              # declared only — invisible scalar
export EU               # exported, never assigned — still a cell
declare -i di; declare -r rr
g() { local -a la; local lb; b '${!la}'; b '${!lb}'; }
for p in s t e0 d0 z EU di rr; do b "\${!$p}"; b "\${!$p:-x}"; b "\${!$p+S}"; done; g
# bash: <> st=0 / <x> st=0 / <> st=0 for every one of them
# osh : "<p>: invalid indirect expansion", alive=1, for every one of them
```

The complaint is about the *pointer*, never about what it points at — a pointer
that reads fine but names nothing is already an unset parameter in both shells,
so only the rows above diverge:

```sh
d0=one                  # `one' does not exist
b '${!d0}'; b '${!d0:-x}'   # both shells: <> st=0 / <x> st=0
```

and the mirror-image case, a nameref with nothing in it:

```sh
declare -n nr
b '${!nr}'      # bash: "nr: invalid indirect expansion"   osh: <nr>
b '${!nr:-x}'   # bash: same complaint                      osh: <nr>
```

**Why bash does this.** `parameter_brace_expand_indir` (subst.c) tests the
*variable*, not its value:

```c
if (var_is_special == 0 && (v = find_variable_last_nameref (name, 0)))
  { … return the nameref's cell if it has one … }

if (legal_identifier (name) && v == 0)
  {
    report_error (_("%s: invalid indirect expansion"), name);
    …
  }

t = parameter_brace_find_indir (name, var_is_special, quoted, 0);
…
if (t == 0 && valid_array_reference (name, 0))
  {
    v = array_variable_part (name, 0, (char **)0, (int *)0);
    if (v == 0)
      { report_error (_("%s: invalid indirect expansion"), name); … }
    else
      return (WORD_DESC *)NULL;
  }

if (t == 0)
  return (WORD_DESC *)NULL;      /* the whole ${!x…} is an unset parameter */

if (valid_brace_expansion_word (t, SPECIAL_VAR (t, 0)) == 0)
  { report_error (_("%s: invalid variable name"), t); … }
```

So the complaint is for a name that has **no `SHELL_VAR` at all**. A declared-
but-empty array, and even an invisible `declare z`, has one — `find_variable_last_nameref`
finds it — and then `parameter_brace_find_indir` reads `${z}`, gets NULL, and the
indirection returns NULL, which every operator downstream treats as an unset
parameter. An unset *nameref* goes the other way: it has no cell, so the first
clause does not return, and `v` is 0 for the invisible variable a bare
`declare -n nr` makes, so it does hit the complaint.

The `legal_identifier` guard is why a *subscripted* pointer takes the second
door instead: `${!nope[k]}` is not an identifier, so it falls through to the
array clause, which asks the same question of the array part alone — `nope` has
no cell, so it complains and quotes the whole `nope[k]`; `${!e0[0]}` through a
declared-but-empty `e0` finds the cell and is an unset parameter. Note that this
clause is reached only when reading the pointer produced *nothing*: an existing
element whose value happens to name nothing (`${!el[k]}` with `el[k]='n[9]'`)
never gets here.

**The nameref half, in bash's own words.** `find_variable_last_nameref` hands
back the *last link it followed*, which is why a chain whose end names nothing
still points — `declare -n n=nosuch` yields `nosuch`, and `declare -n n7=tgt`
keeps yielding `tgt` after `unset tgt`. A reference with an **empty cell** ends
the walk differently (variables.c):

```c
newname = nameref_cell (v);
if (newname == 0 || *newname == '\0')
  return ((vflags && invisible_p (v)) ? v : (SHELL_VAR *)0);
```

and every question an indirection asks goes through `vflags == 0`, so the answer
is a null variable and the name is treated as though it did not exist at all.
`declare -n n1`, a chain that reaches one (`declare -n n3=n1`), a `local -n l1`,
and a local reference that names *itself* (`local -n r=r`) all end there — and
each is `invalid indirect expansion` named after the reference the walk
**started** from, never the one it stopped at. A subscripted reference through
one takes the array door and quotes the whole `n3[0]`.

**Fixed** by splitting the two questions osh asked as one. `ref_name_exists` now
answers "does the pointer name a variable that exists (including an invisible
declaration)?" — `has_stored_binding || named_param_is_set` — and
`indirect_pointer_value`'s `ElemValue::Absent` arm complains only when that is
`no` *and* `is_valid_name` holds, so reading a real cell that yields nothing is
an ordinary unset parameter. The nameref half is a new predicate,
`Shell::ref_walk_stopped_on_a_reference` (the walk ended on a name that still
carries the attribute — osh's spelling of an empty `nameref_cell`), consulted by
`ref_name_exists`, by the silent recogniser `indirect_whole_array`, and by the
new `Shell::nameref_pointer_spelling`, which is now the single home of the
"bare `${!ref}` through a nameref" answer that `expand_indirect`,
`expand_dynamic_with` and `arith_indir_resolves` each used to spell out.
The empty-string case (`mt=''`) keeps its own separate ending,
`": invalid variable name"`. Corpus:
`an-indirect-pointer-asks-only-that-the-variable-exist.sh` and
`a-reference-with-no-cell-points-nowhere.sh`; unit tests
`an_indirect_pointer_asks_only_that_the_variable_exist` and
`a_reference_with_no_cell_points_nowhere`.
