### TD-OILS-UNSET-THROUGH-TEMP-SHADOW. `unset` inside an assignment prefix's scope kills the name instead of revealing what it shadows — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the `unset` builtin. osh has
no scope for the prefix binding: it is a plain `vars` entry that the
command's teardown puts back, so `unset` removes the entry outright and
the *outer* value stays hidden until the teardown restores it.

bash removes the binding from the prefix's own scope, so what was
underneath is revealed immediately:

```sh
( q=1; q=2 eval 'unset q; echo "[${q-UNSET}]"'; echo "[${q-UNSET}]" )
#   bash [1] [1]        osh [UNSET] [1]
( SECONDS=100 eval 'unset SECONDS; echo "[${SECONDS-UNSET}]"' )
#   bash [0]  (the shell's own clock is back)   osh [UNSET]
( BASH_SUBSHELL=9 eval 'unset BASH_SUBSHELL; echo "[${BASH_SUBSHELL-UNSET}]"' )
#   bash [1]                                    osh [UNSET]
```

Note this is not specific to the computed names — the plain `q` case
diverges too, so it is the scope that is missing, not the dynamic hook.

**Proper fix.** Give `Shell::temp_shadow` the saved value alongside the
name (a `VarSnapshot`, as `local_frames` already holds) rather than
leaving it in the caller's parallel `saved` vector, and have `unset` of a
name bound by the innermost prefix scope restore that snapshot and drop
the entry — the "reveal" — instead of unsetting. The teardown then has
nothing left to do for that name. Care is needed for a computed name: the
reveal must also *not* set `dyn_unset`, or the value function is lost for
good.

**Fixed.** Exactly as prescribed. `Shell::temp_shadow` became
`Vec<Vec<(String, Option<Str>)>>`: each prefix scope now carries what its
binding displaced, so the scope *owns* the save/restore and the parallel
`saved: Vec<(String, Option<Str>)>` vectors in `call_function` and
`run_builtin_body` — and their restore loops — are gone, replaced by
`push_temp_shadow`/`pop_temp_shadow`. The reveal is
`Shell::reveal_temp_shadow(name)`: it finds the *innermost* scope binding
the name (that is the one a lookup finds, so the one `unset` takes away),
removes the entry and writes the displaced value back. `unbind_var` calls
it **last**, after everything else it clears, so the reveal outranks the
clearing; and because the name never reaches `drop_dynamic_binding` (see
TD-OILS-UNSET-LOCAL-KILLS-DYNVAR) `dyn_unset` is untouched and the value
function survives.
