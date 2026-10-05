### TD-OILS-UNSET-LOCAL-KILLS-DYNVAR. `unset` of a `local` of a computed name takes the global's value function with it — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the `unset` builtin's
dynamic-special arm (`drop_dynamic_binding`). It records the name in
`dyn_unset`, which is a *global* fact, so unsetting a local of the name
destroys the shell's own binding permanently.

```sh
f() { local SECONDS=9; unset SECONDS; echo "[${SECONDS-UNSET}]"; }
f                 # both [UNSET]  — right
echo "[${SECONDS-UNSET}]"
#   bash [0]  (the clock is untouched)      osh [UNSET]
```

**Proper fix.** `unset` of a name that is `is_ordinary_shadowed` unsets
the *shadow* and must not touch the dynamic binding at all: the value
function belongs to the global, which the frame is standing in front of,
not to the local. So the `dyn_unset` insertion needs the same guard the
read and write paths just got.

**Fixed.** `unbind_var` now snapshots
`let shadowed = self.is_ordinary_shadowed(name);` **before** it removes
anything — it has to be decided first, because the temp-shadow reveal at
the end takes the prefix's binding away — and calls
`drop_dynamic_binding` only `if !shadowed`. So unsetting a `local` (or a
prefix binding) of a computed name removes the shadow and leaves the
global's value function intact, while unsetting the name itself still
records it in `dyn_unset` as before.

Both fixes are covered by the "unset empties the scope" section of
`tests/corpus/bash-assign-prefix-dynvar.sh` and by five assertions at the
end of the unit test `an_assignment_prefix_shadows_a_computed_name`.
