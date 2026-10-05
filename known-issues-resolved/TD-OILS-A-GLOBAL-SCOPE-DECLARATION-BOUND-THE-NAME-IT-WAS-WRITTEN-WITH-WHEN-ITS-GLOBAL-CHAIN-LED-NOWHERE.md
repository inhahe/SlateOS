### TD-OILS-A-GLOBAL-SCOPE-DECLARATION-BOUND-THE-NAME-IT-WAS-WRITTEN-WITH-WHEN-ITS-GLOBAL-CHAIN-LED-NOWHERE. `declare -g g=1` under a dead global nameref chain wrote the frame's own reference — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::global_bind_names`] (was
`global_bind_name`) and the new [`Shell::global_chain_path`]. A declaration made
at global scope from inside a function does not always bind the name it was
written with. bash decides in two questions (`do_compound_assignment`,
subst.c:3494):

```c
v = chklocal ? find_variable (name) : 0;
if (v && (local_p (v) == 0 || v->context != variable_context)) v = 0;
if (v == 0) v = find_global_variable (name);
…
newname = (v == 0) ? nameref_transform_name (name, flags) : name;
```

`find_global_variable` (variables.c:2400) answers NULL three ways — no global
binding, a chain ending on an unset name, and a chain that closes on itself or
outruns `NAMEREF_MAX`. Where it does, the name bound is the **live** chain's
last reference cell (`find_variable_last_nameref`), bound globally all the same.
So a frame-local reference pointing out of the frame decides which global a
`declare -g` writes, whenever the global side of the lookup is empty. osh took
the global side's answer only, and so bound the name as written.

The builtin half reaches the same rule two steps later:
`find_global_variable_last_nameref` (declare.def:735) walks the global table
*alone*, so a cycle leaves it with nothing and the name falls through unchanged
to `bind_global_variable`, whose `bind_variable_internal` (variables.c:3020)
opens with the same live-chain cell. That opening is reached only through the
global table's own entry for the name — which is the one thing the two roads
agree on: a name with **no global binding at all** is bound exactly as written.

**Reproduce.**

```sh
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; declare -p g z w )
# bash: declare -- w="1"   osh (before): the value on g's own chain
```

**The fix.** Two changes. `global_bind_names` gained the missing arms — no
global binding at all binds the name as written; a global chain reaching the
live binding of the name itself is no swap; a chain leading nowhere takes the
live chain's last cell for the compound-literal road, and the *global-only path*
for the builtin road. And because osh's swap can only *un-shadow* a name — it
cannot redirect a write — the builtin road returns the whole path
([`Shell::global_chain_path`]), so the ordinary innermost-first walk is put onto
the global chain and reaches the same cell bash names.

**Tests.** Unit test
`a_global_scope_declaration_whose_global_chain_dies_binds_the_live_chains_last_reference`
and corpus
`a-global-scope-declaration-whose-global-chain-dies-binds-the-live-chains-last-reference.sh`
(10 sections). Two probe matrices went 25/27 and 48/56 — the remainder is
TD-OILS-A-NAMEREF-CHAIN-THAT-OUTRUNS-THE-LINK-LIMIT-IS-TAKEN-FOR-ONE-THAT-CLOSED.
