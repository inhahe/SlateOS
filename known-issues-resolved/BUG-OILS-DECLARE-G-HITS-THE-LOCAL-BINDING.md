### BUG-OILS-DECLARE-G-HITS-THE-LOCAL-BINDING. `declare -g` inside a function operates on the local binding instead of the global one — 2026-07-30 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare`. `make_local` is
computed as `is_local || (!global && !self.local_frames.is_empty())`, so `-g`
correctly suppresses the *local shadowing* step — but nothing then redirects the
declaration's effects to the global binding. Every subsequent step (the array
kind, the attributes, the value) is applied to whatever `self.vars`/`arrays`
currently resolve to, which inside a function is the local shadow.

**What.** `declare -g` means "act on the global variable of this name, even
inside a function". When a local of the same name exists, bash's `-g` reaches
*past* it. osh converts the local instead, and the global is never touched.

```sh
f(){ local x=5; declare -ga x; declare -p x; }; f
echo "after:"; declare -p x
```

```
bash                          osh
declare -- x="5"              declare -a x=([0]="5")
after:                        after:
declare -a x                  osh: line 1: declare: x: not found
```

bash makes the *global* `x` an empty array and leaves the local scalar alone, so
`declare -p x` inside the function still shows the local `x="5"` and the global
array survives the return. osh converts the local, so the array is discarded with
the frame and no global is ever created. Note the two shells agree when no local
shadow exists (`f(){ x=5; declare -ga x; }` and a pre-existing global both give
`declare -a x=([0]="5")`), which is why this went unnoticed.

**Proper fix.** `-g` has to *target* the global binding, not merely skip the
shadowing step. The frame stack records the shadowed value for each local name
(`snapshot_var`), so the global for a shadowed name is reachable — a `-g`
declaration should write the outermost binding: update the frame's saved snapshot
(so the value survives the return) rather than the live `vars`/`arrays` entry
when a local shadow is in the way. Found while probing
TD-OILS-LOCAL-ARRAY-KIND-VALUED; kept separate because it is a scoping bug, not a
display one, and the fix touches the frame representation.

**Fixed 2026-07-31.** Rather than reimplement every declaration effect (array
kind, `-i`/`-l`/`-u`/`-c`/`-n`/`-x`/`-r`, scalar value, `+=` append, compound
literal) a second time against a `VarSnapshot`, the fix makes the global binding
*live* for the duration of the declaration and lets the ordinary code run against
it. `Shell::enter_global_scope(names)` finds, for each operand, the **outermost**
frame that shadows the name — that frame's snapshot is the true global, which is
why a `-g` two calls deep still writes the global and not the enclosing
function's local — swaps it into `vars`/`arrays`/the attribute sets via the
existing `restore_var`, and keeps the displaced local.
`Shell::leave_global_scope` then re-snapshots the name (now holding whatever the
declaration made of the global), writes it back into that frame's slot so it
survives the return, and restores the local. A name with no shadow is left alone:
it is already global.

The swap has to wrap the **whole command**, not just the builtin, because a
compound operand (`declare -ga x=(7 8)`) is bound during the word-expansion pass
in `exec_declare_with_arrays` phase 1 and never reaches `builtin_declare` at all.
Both entry points are therefore thin wrappers — `builtin_declare` /
`exec_declare_with_arrays` — over the unchanged bodies, renamed
`*_scoped`, which now simply run with the right bindings already live. Phase 2
calls `builtin_declare_scoped` directly so the outer swap is not re-entered (a
nested enter/leave pair would restore the *outer* saved binding over the inner
result). `local` never swaps, whatever flags it is given. Two small helpers,
`declare_global_flag` and `declare_operand_names`, do the pre-scan the wrappers
need; they are separate from the builtins' own flag loops because the swap must
already be in force when the first operand is processed.

Verified against bash 5.2.37 over 22 cases — local scalar/array shadow, existing
vs. absent global, value / attribute-only / `+=` / compound-literal / `-A` / `-r`
/ `+i` declarations, `typeset -g`, two-frame nesting, several operands at once —
all byte-identical apart from the shell name in one diagnostic. Covered by
`declare_g_reaches_past_a_local_to_the_global_binding`.

**Not replicated (a bash quirk, deliberately).** With a *global indexed array*
and a local scalar shadow, `declare -g x+=9` makes bash print
`declare -a x=([0]="59")` from inside the function while leaving the global array
`([0]="1" [1]="2")` untouched on return — it appends to the local's value but
reports it under the global's type, and writes neither binding coherently. osh
appends to the global, consistent with every other `-g` case.
