### TD-OILS-DECL-VALUELESS-IGNORES-READONLY. `declare x` / `local x` inside a function shadows a readonly global instead of refusing — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the valueless-operand path of
the declaration builtins. The readonly check is made when an operand
carries a value; a bare name skips it and goes straight to making the
local shadow.

```sh
readonly ro=5
f() { declare ro; echo "rc=$?"; }   # bash: "declare: ro: readonly variable", rc=1
g() { local ro;   echo "rc=$?"; }   # bash: "local: ro: readonly variable",   rc=1
f; g                                # osh: rc=0 twice, and the name is shadowed
```

Reproduce with `target/dvscratch/px29.sh`. bash refuses because
`make_local_variable` will not shadow a readonly binding with a writable
copy, so a function that tries it fails on the spot. That difference is
observable: under `set -e` bash aborts the function where osh runs on.

**Measured rule (bash 5.2.37, `target/dvscratch/px29.sh`–`px34.sh`).**
What matters is *whose* readonly binding the declaration would hide:

* A readonly **global** is unshadowable. Every declaration that would
  make a local of that name is refused — valueless (`declare ro`),
  valued, compound (`local -a ro=(1)`) and `+r` alike — tagged with the
  builtin that asked. The compound form reports twice, like the
  `noassign` refusal: once from the assignment machinery (tagged with the
  running function) and once from the builtin.
* A readonly **local of an outer frame** is shadowed freely: the new
  local is a separate writable binding, so the value, the flags and later
  writes all land on it, and the outer readonly is back on return
  (`f() { local -r y=1; g; }; g() { local y=9; y=7; }` succeeds).
* A name **this frame already holds** is a re-declaration, not a shadow,
  so the operand meets the readonly binding itself: the valueless form
  succeeds (an attribute update), anything that writes it is refused.
* Nothing that reaches the global rather than hiding it is refused —
  `declare -g ro`, `export ro`, `readonly ro` inside a function and every
  valueless declaration at top level all succeed.

**Resolved 2026-07-31.** `builtin_declare_scoped` computes
`held_here`/`held_outer`/`shadow_new` and folds them into one
`readonly_blocks` predicate now shared by the valued, the `+r` and the
new shadow refusal; `exec_declare_with_arrays_scoped` gained the matching
double-reported refusal for compound operands; and `declare_local` clears
the readonly marking along with the other attributes, so the shadow it
makes really is writable (the snapshot restores it when the frame goes).
Covered by `a_local_shadows_a_readonly_local_but_not_a_readonly_global`
and two sections of `userspace/oils/tests/corpus/readonly-attr.sh`.
