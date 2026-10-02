### TD-OILS-LOCAL-INHERITS-TEMP-BINDING. A valueless `local` of a name the caller bound with an assignment prefix starts out unset instead of taking the prefix's value — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `declare_local` for the
valueless case. It makes the name unset for the frame regardless of what
the enclosing prefix scope bound.

Measured (`target/dvscratch/ap5.sh`):

```sh
( f() { local q; echo "[${q-UNSET}]"; }; q=2 f )              # bash [2]    osh [UNSET]
( q=1; f() { local q; echo "[${q-UNSET}]"; }; q=2 f )         # bash [2]    osh [UNSET]
( f() { local SECONDS; echo "[${SECONDS-UNSET}]"; }; SECONDS=100 f )
#                                                              bash [100]  osh [UNSET]
( f() { local q; declare -p q; }; q=2 f )   # bash declare -x q="2"   osh declare -- q
```

It is specifically the **prefix's** scope that is inherited, not the
enclosing value in general: with no prefix in force, `local q` inside a
function where `q=1` globally leaves the name unset for the frame in both
shells (that part osh already gets right). And the `declare -p` line
shows what is really happening — the `local` does not create a new
binding at all, it *promotes the prefix's binding into the frame*, `-x`
and all, so the value comes along with it.

Only the valueless form is affected; `local NAME=v` obviously writes `v`.

**Proper fix.** In `declare_local`, when an assignment-prefix scope
(`Shell::temp_shadow`) binds the name, the valueless form should adopt
that binding — value and attributes — rather than starting the frame with
the name unset. Best done together with
TD-OILS-TEMP-BINDING-ATTRIBUTES, since both need the prefix scope's saved
entry to carry attributes as well as the value.

**Fixed**, and it turned out to be two halves rather than one. Measuring
further (`target/dvscratch/ap6.sh`–`ap8.sh`) showed the `local` really is
a *second* binding layered on the prefix's, not a promotion of it: an
`unset` of it leaves the name unset for the frame and the prefix's
binding is still there for the rest of the call —

```sh
q=1; f() { local q; unset q; echo "[${q-UNSET}]"; }; q=2 f; echo "[$q]"
#   [UNSET] then [1]   — inside f, q is unset; the prefix binding is gone with f
q=1; f() { g; echo "[$q]"; }; g() { local q; unset q; }; q=2 f
#   [2]  — g's local was the nearer of the two, so the prefix's survived it
```

So `Shell::temp_shadow` became `Vec<TempScope>`, each scope recording the
`local_frames.len()` it was opened at. A local frame at that index or
deeper is *inside* the scope, which answers both halves:

- `declare_local` inherits the value (and the `-x` with it) when a scope
  opened at or before this frame binds the name — `Shell::temp_binds`.
  The value is taken as it stands, so `local -i q` under `q=3+4 f` still
  reports `declare -ix q="3+4"`; the declaration's own flags are applied
  on top afterwards, as for any `local`.
- `reveal_temp_shadow` does nothing when a local frame inside the scope
  binds the name: the `unset` took the local, and the prefix's binding
  stays hidden underneath.

Covered by the "a valueless local of one keeps the value" section of
`tests/corpus/bash-assign-prefix-dynvar.sh` and by seven assertions in
`an_assignment_prefix_shadows_a_computed_name`.
