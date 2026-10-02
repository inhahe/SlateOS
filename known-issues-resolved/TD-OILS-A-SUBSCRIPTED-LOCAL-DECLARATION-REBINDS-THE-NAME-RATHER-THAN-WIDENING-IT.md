### TD-OILS-A-SUBSCRIPTED-LOCAL-DECLARATION-REBINDS-THE-NAME-RATHER-THAN-WIDENING-IT. `local g=5; local g[1]=9` kept the scalar as element 0 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped` —
`drop_local_scalar`, which asked only about `-a`/`-A`.

A subscript asks a declaration for an array as plainly as `-a` does:
declare.def:614 sends both spellings down the one call, and
`making_array_special` is set for any subscripted operand, valued or not.

```c
  else if ((flags_on & att_array) || making_array_special)
    var = make_local_array_variable (newname, MKLOC_ASSOCOK|inherit_flag);
```

`make_local_array_variable` **rebinds** the name as a fresh array rather than
widening the variable in place, so the scalar the frame already held is lost:

```sh
f() { local g=5; local g[1]=9; declare -p g; }; f   # declare -a g=([1]="9")
f() { local g=5; local g[1];   declare -p g; }; f   # declare -a g=()
```

osh gave `([0]="5" [1]="9")` and `([0]="5")`. `declare` and `typeset` bind
locals inside a function too and were wrong the same way.

Three neighbours are **not** this, and were already right: at global scope the
widening is `find_or_make_array_variable`, which converts the variable and keeps
its value as element 0 (`g=5; declare g[1]=9` is `([0]="5" [1]="9")`); a scalar
bound at an *outer* context is merely shadowed and survives the return; and an
array already of the asked-for kind is not rebound at all.

**Corpus:**
`a-subscripted-local-declaration-rebinds-the-name-rather-than-widening-it.sh`.
Unit test:
`a_subscripted_local_declaration_rebinds_the_name_rather_than_widening_it`.
