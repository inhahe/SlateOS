### BUG-OILS-DECL-NAMEREF-LOCAL-SCOPE. A `declare`/`local` that binds a local followed a nameref it was only shadowing — 2026-07-31 — ✅ **RESOLVED 2026-07-31**

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`, the
`follow` condition, which asked only `self.nameref_attr.contains(base_name)`:
"does this name resolve to a reference from here?" rather than "is the
binding this declaration writes a reference?"

**Reproduce** (found while measuring TD-OILS-DECL-NAMEREF-SUBSCRIPT):

```sh
w=5; declare -n r=w
f() { declare r=9; declare -p r; }; f     # bash declare -- r="9"   osh declare -n r="w"
                                          # …and osh made a *local* w=9 instead
f() { declare -i r; declare -p r; }; f    # bash declare -i r       osh declare -n r="w"
```

A declaration inside a function binds a **local**, and a global nameref of
the same name is simply what that local shadows: bash gives the function a
fresh ordinary local and leaves the target alone. osh instead resolved
through the global reference and declared a local of the *target*, so both
the reference and the variable the caller expected to see were wrong.

The damage reached further than the local frame. Because `local r=1` had
already localised `w` rather than `r`, a following `declare -g r=9` — which
*should* follow the global reference — wrote into that local `w` and the
store vanished when the frame popped:

```sh
v=5; declare -n s=v
g() { local s=1; declare -g s=9; }; g; echo "$v"   # bash 9   osh 5
```

**The rule** (measured against bash 5.2.37, probes `nr20`–`nr25`): a
declaration follows a reference iff **the binding it is about to write is
itself a reference**.

* A local-binding `declare`/`local`/`typeset` writes the *innermost* frame's
  binding — so it follows only when that frame already binds the name as a
  reference (`f() { local -n r=w; local r=9; }` does follow, into a local
  `w`). A binding a *caller* made does not count.
* A `-g` (or global-scope) declaration writes the *global* binding, and
  consults that one even when a local hides it.
* `export`/`readonly` bind no local at all, so they always follow.

**Fixed.** New helper `Shell::local_binds_here` (beside
`is_locally_shadowed`, which asks about *any* frame) answers the innermost
question; `follow` now also requires `!make_local || local_binds_here(…)`.
The `-g` half needs no test of its own — `enter_global_scope` has already
swapped the global binding into the live tables for the whole builtin, so
`nameref_attr` there *is* the global's. Covered by a new section in
`tests/corpus/nameref-declare.sh` and the unit test
`a_declaration_follows_a_reference_only_when_its_own_binding_is_one`.
