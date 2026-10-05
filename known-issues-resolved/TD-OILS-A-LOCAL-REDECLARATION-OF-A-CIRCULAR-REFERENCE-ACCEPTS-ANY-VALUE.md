### TD-OILS-A-LOCAL-REDECLARATION-OF-A-CIRCULAR-REFERENCE-ACCEPTS-ANY-VALUE. `local g=5` on a circular local nameref wrote `5` into the reference where bash refuses it — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped` — the
follow walk, the `follow && target.is_none()` branch, the readonly/un-assignable
refusals, and a new refusal ahead of the array ones.

**What it was.** `f() { local -n g=z; local -n z=g; local g=5; }` left
`declare -n g="5"` with status 0; bash refuses the value, keeps
`declare -n g="z"` and exits 1. Ten of twenty-one measured declaration forms
differed, all of them function-local declarations *with a value* through a chain
that did not end on a name.

**The measurement that explains it.** A local declaration never re-scopes onto
whatever its chain found. The name it declares comes from
`declare_transform_name` (declare.def:205):

```c
  v = find_variable_last_nameref (name, 1);
  newname = (v && v->context != variable_context) ? name : name_cell (var);
```

— `name_cell (var)`, the found variable's **name**, not its value. A cycle
escapes through `find_global_variable_noref (v->name)` (variables.c:2104), whose
`v->name` is the operand's own name again, and a chain past the link cap finds
nothing at all. Either way `make_local_variable` hands back the frame's own
reference unchanged, so what follows is a declaration *about a nameref*, and
declare.def:817 refuses a value that names nothing:

```c
  else if (nameref_p (var) && (flags_on & att_nameref) == 0 &&
           (flags_off & att_nameref) == 0 && offset &&
           valid_nameref_value (value, 1) == 0)
    { builtin_error (_("`%s': invalid variable name for name reference"), value);
      any_failed++; NEXT_VARIABLE (); }
```

The refusal quotes the operand's **raw** right-hand side, so an append says
`` `5' `` and not the joined `z5`; it judges the value as *written*, so
`local -u g=ab` passes and is folded to `AB` afterwards while `local -u g='a b'`
is refused; and an empty value takes this wording rather than the generic "not a
valid identifier".

Where it sits in the loop was measured too. It is **behind** the array kind,
which `make_local_array_variable` applied back at declare.def:614 — hence
`local -a g=5` leaving `declare -an g=()` — and **ahead** of every refusal after
it:

| through the cycle | bash says |
|---|---|
| `local -a g; local +a g=5` | `` `5' ``, not "cannot destroy array variables" |
| `local -a g; local -A g=5` | `` `5' ``, not "cannot convert indexed to associative" |
| `local -A g; local -a g=5` | `` `5' ``, leaving `declare -An g=()` |
| `local -r g; local g=5` | `` `5' ``, not "readonly variable" |

The question is only ever about the variable, never about how the chain failed:
a reference whose value cell an `ARRAY *` has overwritten follows nothing and
still refuses. At **global** scope there is no such variable to find — the
widening is `find_or_make_array_variable`, which takes the reference attribute
off — so `declare -a g; declare +a g=5` on a global cycle really does say
"cannot destroy".

**The fix.** The follow walk now calls `walk_ref_name` directly and sets
`local_ref_fallback` when a `make_local` walk did not end on a name, which drops
the escape and leaves the operand's own name at live scope (the
`follow && target.is_none()` branch is skipped for it, so a valueless
`local -i g` still gives `declare -in g="z"` and `local -a g` still gives
`declare -an g=()`). `ref_value_refused` is then decided beside `held_here` —
`make_local_variable`'s own answer, since a name the frame already holds comes
back as the binding it has where a fresh local is made anew — early enough for
the readonly and un-assignable refusals to stand aside, and the refusal itself
is emitted after the array kind and before the array refusals.

**Not this, and unchanged.** A chain that reaches a name (`local w=1;
local -n g=w; local g=5` binds `w`), one that reaches a name which does not
exist (`local -n g=nosuch`), a fresh local shadowing a cycle that lives at
global scope, `-g`/`readonly`/`export` (all off the local branch), and top-level
declarations.

**Found alongside:**
TD-OILS-A-PLUS-N-DECLARATION-STORES-THROUGH-THE-REFERENCE-IT-IS-REMOVING and
TD-OILS-AN-ARRAY-REFUSAL-MAKES-AN-UNVALUED-ARRAY-VALUED.

**Corpus:**
`a-local-redeclaration-of-a-circular-reference-refuses-a-value-that-is-not-a-name.sh`.
Unit test:
`a_local_redeclaration_of_a_circular_reference_refuses_a_value_that_is_not_a_name`.
