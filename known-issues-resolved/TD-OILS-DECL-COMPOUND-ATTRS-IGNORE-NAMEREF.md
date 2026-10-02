### TD-OILS-DECL-COMPOUND-ATTRS-IGNORE-NAMEREF. A compound `name=(…)` operand bound *through* a reference left every attribute on the reference — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` —
`exec_declare_with_arrays_scoped`, whose phase 1 and phase 3 both used the
operand's own `a.name` for every table it touched. `apply_assignment`
resolves the reference on its own, so the *values* already reached the
target; nothing around the store did.

The scalar path (`builtin_declare_scoped`) had the resolution right, so
this was a compound-operand-only divergence — measured in
`target/dvscratch/px59.sh`, `px60.sh`, `px61.sh`, `px63.sh`, `px64.sh`.

```sh
declare -A t=([k]=v); declare -n r=t; declare -A r=(z)
  # bash declare -A t=([z]="" ) / declare -n r="t"
  # osh  declare -A t=([k]="v") / declare -An r=([z]="" )
t=1; declare -n r=t; declare -i r=(2+3)
  # bash declare -ai t=([0]="5")   / declare -n r="t"
  # osh  declare -a  t=([0]="2+3") / declare -in r="t"
t=1; declare -n r=t; declare -x r=(5)
  # bash declare -ax t=([0]="5") / declare -n r="t"
  # osh  declare -a  t=([0]="5") / declare -nx r="t"
declare -n r=t; declare -a r=(z)        # bash makes t; osh: declare: t: not found
```

The kind letters were the worst of it: with `-a`/`-A` the *literal* bound
to the reference's own name too, so the target was left untouched and the
reference became an array that was also still a reference.

**✅ RESOLVED 2026-07-31.** Phase 1 now resolves the operand once, at the
head of the loop, and both phases work from the resolved name. The rules,
measured against bash 5.2.37:

* The reference is followed only when the binding this declaration
  *writes* is itself the reference — the same `binding_is_nameref` rule
  the scalar path already states. A local-binding `declare`/`local` in a
  frame that does not hold the reference merely shadows a global one and
  makes an ordinary array of its own.
* Everything around the store follows: the local shadow (so a local
  reference onto a global makes the *target* a local, which vanishes at
  return), the array kind, the value attributes the literal binds under,
  and the `-x`/`-r`/`-t` phase 3 applies. `+n` is the exception it always
  was — it drops the attribute from the reference, by name.
* The diagnostics split. The two refusals that turn on *where the binding
  would land* — a shell-maintained name and a readonly global, both as a
  local — name the target, and lose the function tag the direct form
  carries, because the refusal is raised from the resolution rather than
  from the operand word. Every refusal the builtin raises against the
  operand it was handed names it as written (`declare -n r=t; declare -aA
  r=(z)` reports `r`), as does the untagged conversion refusal.
* A reference that resolves to an *element* (`declare -n r='a[1]'`) names
  no variable an array literal can bind to: bash reports
  `` `a[1]': not a valid identifier `` untagged and discards the rest of
  the parse unit.
* A cycle warns once and falls back to the name the operand was written
  with, which stops being a reference — no variable is both.

Corpus: `userspace/oils/tests/corpus/declare-compound-nameref.sh`. Unit
test: `a_compound_operand_follows_a_reference_with_its_attributes`.

The scalar path's half of the family is
TD-OILS-DECL-SCALAR-REFUSAL-NAMES-TARGET, below.
