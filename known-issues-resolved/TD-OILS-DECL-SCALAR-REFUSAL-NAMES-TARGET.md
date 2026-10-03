### TD-OILS-DECL-SCALAR-REFUSAL-NAMES-TARGET. Two of the scalar path's refusals named the reference's target rather than the operand — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`, whose
diagnostics all used the resolved `base_name`. Only the `+a`/`+A` destroy
refusal already had the `operand_name` it needed.

```sh
readonly t=1; declare -n r=t; declare r=9
  # bash declare: r: readonly variable   osh declare: t: readonly variable
declare -A t=([k]=v); declare -n r=t; declare -a r=z
  # bash declare: r: cannot convert…     osh declare: t: cannot convert…
```

**✅ RESOLVED 2026-07-31.** Measured in `target/dvscratch/px65.sh`,
`px66.sh`, `px67.sh`, `px68.sh`. bash raises these refusals in two
different places, and only the name they quote tells them apart:

* Against the **word the command was handed** — so the operand as written:
  the assignment's readonly refusal, the kind conversion, and the destroy
  refusal.
* Against the **variable a lookup found** — so the resolved target: the
  local-shadow readonly refusal, `+r`, and `variable may not be assigned
  value`.
* The `-aA` self-conflict is *both*, and which one it is depends on
  whether the array it refuses was already there: `declare -A t; declare
  -n r=t; declare -aA r` reports `r`, while the same command on an unset
  `t` reports `t`, because there the array is one this very command made.
  A compound literal binds during word expansion, so its array is always
  one the builtin found — which is why the compound path reports the
  operand as written unconditionally.
