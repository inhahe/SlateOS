### TD-OILS-A-PLUS-N-DECLARATION-STORES-THROUGH-THE-REFERENCE-IT-IS-REMOVING. `local +n g=5` on a circular reference bound `5` where bash refuses the store and keeps the reference — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped` — a
new `ref_store_refused` predicate beside `ref_value_refused`, the `unset_nameref`
removal it holds back, and the refusal itself beside `nameref_int_refusal`.

**What it was.** `f() { local -n g=z; local -n z=g; local +n g=5; }` left
`declare -- g="5"` with status 0; bash refuses the value, keeps
`declare -n g="z"` and exits 1. osh's `+n` route took the attribute off and
then bound the value as an ordinary scalar — there was no counterpart to
bash's *store* check at all.

**The measurement that explains it.** `+n` is excluded from the
declaration-time nameref check outright, by `(flags_off & att_nameref) == 0`
(declare.def:817) — so the complaint about a value that names nothing comes
from the **store** instead, which bash still makes with the reference in force
(declare.def:982):

```c
      if (onref || nameref_p (var))
	aflags |= ASS_NAMEREF;
      v = bind_variable_value (var, value, aflags);
      if (v == 0 && (onref || nameref_p (var)))
	{
	  if (valid_nameref_value (value, 1) == 0)
	    sh_invalidid (value);
	  assign_error++;
	  …
	  flags_on |= onref;		/* undo change from above */
	  flags_off |= offref;
```

The removal is *deferred* past that store — `VUNSETATTR (var, offref)` is at
declare.def:1034 — so the `NEXT_VARIABLE ()` here never reaches it and the
reference survives the refusal intact. `flags_off |= offref` on the line above
puts the removal back for good measure. The wording is the generic
`sh_invalidid` one, `` `%s': not a valid identifier ``, **not** the
nameref-specific line that `-n` and the plain spelling give.

| | bash 5.2.37 | osh (before) |
|---|---|---|
| diagnostic | `` local: `5': not a valid identifier `` | none |
| status | 1 | 0 |
| `declare -p g` | `declare -n g="z"` | `declare -- g="5"` |

Every other letter on the operand has already landed by the time the store
fails, because the flags are applied at declare.def:944 — so `local +n -x g=5`
leaves `declare -nx g="z"`, and `local +n -r g=5` a readonly one. Only the
operand that carried the bad value is dropped; those after it still store.

**Four limits, all measured.** The declaration must be the one that *found*
the frame's reference, so a fresh local shadowing a cycle that lives at global
scope is not this (`held_here`). A chain whose target does not exist is not
this either — `declare_transform_name` falls to `newname = name` only when
`find_variable (name) != 0`, so `local -n g=nosuch; local +n g=5` re-scopes the
whole declaration onto `nosuch` and binds it (`unset_nameref_follows`). An
**append** is never validated, because it goes on the reference's own text:
`local +n g+=5` through a cycle leaves `declare -- g="z5"`. And the array
spellings are held out: bash refuses those too, but the name it refuses it
*built*, from a buffer the array path never filled — see
TD-OILS-DECL-UNSET-NAMEREF-BASH-CORNERS.

**The fix.** `ref_store_refused` is computed beside `ref_value_refused`, gated
on `unset_nameref && make_local && held_here && !nameref &&
!unset_nameref_follows && !append`, no subscript or array letter, the name
carrying the reference attribute, and a value that is not a name. Where it
holds, the `unset_nameref` removal is skipped (the deferral at :1034), the
generic `` `%s': not a valid identifier `` is emitted with the builtin's own
tag, status is set to 1, and the operand's value is dropped — shaped exactly
like `nameref_int_refusal` beside it, so the flags still apply and the
following operands are still read.

**Not this, and unchanged.** A value that *is* a name is stored through the
reference and the removal then stands (`local +n g=h` retargets and leaves
`h`); `local +n g="a[1]"` likewise; a valueless `local +n g` makes no store to
fail; a readonly reference is refused earlier and says so differently;
`-g`/`export`/`readonly` are off the local branch; and top-level declarations
have no local to find.

**Found alongside:**
TD-OILS-A-LOCAL-REDECLARATION-OF-A-CIRCULAR-REFERENCE-ACCEPTS-ANY-VALUE, whose
31-case sweep turned this up as the one case its declaration-time refusal does
not cover.

**Corpus:**
`a-plus-n-local-declaration-refuses-a-store-through-the-reference-it-is-removing.sh`.
Unit test:
`a_plus_n_local_declaration_refuses_a_store_through_the_reference_it_is_removing`.
