### TD-OILS-A-GLOBAL-ARRAY-DECLARATION-IN-A-FUNCTION-STORES-INTO-WHICHEVER-BINDING-IS-LIVE. `declare -g -a g=9` over a local `g` declares the global an array but puts the element in the local — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped`, the
`assoc || indexed` store branch and the `attr_store` beside it. Both write the
variable the declaration is about, which under `-g` is the global. The
subscripted spelling was split in
TD-OILS-A-GLOBAL-SUBSCRIPTED-DECLARATION-IN-A-FUNCTION-ASSIGNS-TO-WHICHEVER-BINDING-IS-LIVE;
the **whole-array** spelling splits the same way and was missed.

The store is declare.def:970, and it is made by **name**:

```c
      else if (simple_array_assign)
	{
	  /* let bind_{array,assoc}_variable take care of this. */
	  if (assoc_p (var))
	    bind_assoc_variable (var, name, savestring ("0"), value, aflags|ASS_FORCE);
	  else
	    bind_array_variable (name, 0, value, aflags|ASS_FORCE);
	}
```

`bind_array_variable` begins `entry = find_shell_variable (name)`
(arrayfunc.c:266) — an ordinary lookup, where everything above it held the
`find_global_variable` one. The **assoc** half of the same branch is handed
`var` itself, so `-A` does *not* split. `simple_array_assign` is chosen at
declare.def:885 by `(making_array_special || creating_array || array_exists) &&
offset` with a value that is not a `(…)` compound — and `array_exists` is judged
on `var`, i.e. on the **global**, so a global that is already an array makes an
element store out of a flagless `declare -g g=9`.

`ASS_FORCE` is passed, so a readonly live binding does not stop the store and
raises nothing; an assoc live binding is converted (`array_p (entry) == 0` →
`convert_var_to_array`, arrayfunc.c:284), discarding its keys.

**Reproduce.**

```sh
f() { local g=5; declare -g -a g=9; declare -p g; }; f; echo AFTER; declare -p g
```

| | bash 5.2.37 | osh |
|---|---|---|
| inside `f` | `declare -a g=([0]="9")` | `declare -- g="5"` |
| after `f` | `declare -a g=()` | `declare -a g=([0]="9")` |

`readonly -a g=9` and `export -a g=9` reach it too: setattr.def:234 rewrites
them into `declare -gra …` / `declare -gxa …`, adding the `-g` itself — "only
local/declare/typeset create local variables". osh does not make that rewrite
at all, which is its own bug and is tracked separately as
TD-OILS-READONLY-A-AND-EXPORT-A-ARE-DECLARATIONS-WITH-G-ADDED.

Everything the subscripted split measured holds here: the fold is the **live**
variable's (`declare -g -a -i g=4+4` stores the raw `4+4` and leaves the global
`declare -ai g=()`, while `local -i g=5; declare -g -a g=4+4` stores `8`), a
live nameref is followed, and only the operand carrying the array kind splits.

**Fixed** in `c2534fdda`: the indexed element store now runs inside
`Shell::with_live_binding`, the helper the subscripted split already uses,
while the associative one and the compound literal stay on the declaration's
own variable. Two things came out of the measurement that the entry above did
not anticipate:

* the store's `find_shell_variable` **follows a nameref** (`if (var &&
  nameref_p (var)) var = find_variable_nameref (var)`, variables.c), so it is
  routed through `Shell::resolve_ref_array_write` — one walk, matching the one
  `find_variable_nameref` makes, and so one warning on a circular chain. A
  reference naming an *element* has no variable to make out of it
  (`make_new_array_variable (nameref_cell (entry))` is handed `w[1]`), which
  bash reports as `` `w[1]': not a valid identifier `` and stores nothing.
* a chain that **escaped a cycle** names an outer context — the very global
  the declaration just made. osh reaches an outer context *through* the local
  frames, so `with_live_binding` had to park the displaced binding back in the
  frame that shadows it (as `leave_global_scope` does) rather than merely hold
  it aside. `f() { local -n g=g; declare -g -a g=9; }` then stores into the
  global and leaves the local reference standing, which is what bash does.

Corpus: `tests/corpus/a-global-array-declaration-stores-into-whichever-binding-is-live.sh`.

**Found alongside:** TD-OILS-A-DECLARE-ARRAY-STORE-BRANCH-IS-CHOSEN-BY-THE-FLAGS-ALONE.
