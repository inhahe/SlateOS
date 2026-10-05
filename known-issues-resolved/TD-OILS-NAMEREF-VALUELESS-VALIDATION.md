### TD-OILS-NAMEREF-VALUELESS-VALIDATION. `declare -n NAME` on an already-set variable does not validate its value — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `nameref_value_error` covers
only the `declare -n NAME=value` form; the valueless form applies the
attribute unconditionally.

A nameref's target is its *value*, so `declare -n` on a variable that
already has one has to check that value the same way the assignment form
does. bash refuses, with the value quoted in the message, and applies
nothing:

```sh
q=0;  declare -n q; echo "rc=$?"; declare -p q
# bash: declare: `0': invalid variable name for name reference / rc=1 / declare -- q="0"
# osh :                                                          rc=0 / declare -n q="0"
q=zz; declare -n q; declare -p q     # both: declare -n q="zz"   (a valid name, so it stands)
declare -n q;       declare -p q     # both: declare -n q        (unset, so nothing to check)
```

This is a general rule, not a dynamic-variable one, but it is most
visible on the dynamic specials, whose values are numbers:
`declare -n SECONDS` reports `declare: \`0': invalid variable name for
name reference` in bash and leaves `declare -i SECONDS="0"`, where osh
reports nothing and leaves `declare -in SECONDS="0"`. Fix: reuse
`nameref_value_error` against the current value on the valueless path.

**Fixed 2026-07-31.** Not by reusing `nameref_value_error` — measuring
the valueless form against bash 5.2 showed it applies a *different* set
of rules — but with a sibling, `nameref_existing_value_error`, called
from `builtin_declare_scoped`'s operand loop when `-n` is set and the
operand carries no value. It runs **after** the `declare_local` shadow
step (a fresh local holds nothing to judge, which is why `q=0; f() {
declare -n q; }` succeeds) and **before** any attribute is applied (the
refusal abandons the whole operand, so `q=0; declare -rn q` leaves a
plain `declare -- q="0"` with neither letter on it). The rules that
differ from the assignment form:

* **A self-reference is allowed.** `q=q; declare -n q` stands, where
  `declare -n q=q` is an error at global scope — this command supplied
  no value, so there is no circle it could have written.
* **An empty value takes the nameref-specific wording** (`` `':
  invalid variable name for name reference ``), not the generic "not a
  valid identifier" an empty assignment gets.
* **An array is refused outright**, whatever its elements say, with its
  own message: ``declare: q: reference variable cannot be an array``.
  That covers the dynamic specials that list as arrays (`BASH_SOURCE`,
  `BASH_LINENO`); the scalar ones are judged by the value their function
  computes, and once `unset` has taken that function away there is
  nothing left to judge.

Covered by the corpus case `nameref-valueless.sh` and the unit test
`valueless_nameref_declaration_judges_the_stored_value`.
