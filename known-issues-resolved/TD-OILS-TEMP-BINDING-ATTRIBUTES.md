### TD-OILS-TEMP-BINDING-ATTRIBUTES. A prefix binding is reported with the global's attributes, and never as exported — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the temporary-environment
path again. The binding is an entry in `vars` and nothing else, so a
listing reads the *global's* attribute sets for the name, and the export
marking a prefix always carries is missing.

```sh
( q=2 eval 'declare -p q' )                   # bash declare -x q="2"    osh declare -- q="2"
( declare -i q; q=3+4 eval 'declare -p q' )   # bash declare -x q="3+4"  osh declare -i q="3+4"
( SECONDS=100 eval 'declare -p SECONDS' )     # bash declare -x SECONDS="100"
#                                               osh  declare -- SECONDS="100"
```

Two facts in one: a prefix binding is **exported** (it is the command's
environment, so `-x` always), and it is a **fresh** variable that
inherits none of the global's attributes — `declare -i q` does not make
`q=3+4 cmd` evaluate, and the listing shows no `-i`.

**Proper fix.** TD-OILS-UNSET-THROUGH-TEMP-SHADOW has since given the
prefix a real scope holding the displaced value, so the binding can now
be installed the way `declare_local` installs a local — clearing the
attribute sets for the name and adding `exported` — with the scope's
saved entry putting the global's attributes back on teardown. That saved
entry is currently only the *value* (`Option<Str>`); it has to grow to
carry the attribute sets too.

**Fixed.** `Shell::temp_shadow` is now
`Vec<Vec<(String, VarSnapshot)>>` — the same shape `local_frames` uses,
because it is the same kind of scope — so `push_temp_shadow` captures a
full `snapshot_var`, clears the name's array/assoc bindings and every
attribute set (`-i`/`-l`/`-u`/`-c`/`-n`/`-t`/array-valued/`declared`),
writes the value and adds `exported`; `pop_temp_shadow` and
`reveal_temp_shadow` both hand the snapshot to `restore_var`. `readonly`
is deliberately left intact, as in `declare_local`, so a readonly global
is not silently shadowed. `Shell::replace_var` had no callers left and
was removed.

Covered by the "because the binding is a fresh, exported variable of its
own" section of `tests/corpus/bash-assign-prefix-dynvar.sh` and by five
assertions in `an_assignment_prefix_shadows_a_computed_name`.
