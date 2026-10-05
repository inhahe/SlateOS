### TD-OILS-READ-A-AND-MAPFILE-DO-NOT-TAKE-A-NAMEREF-CYCLES-GLOBAL-ESCAPE. A whole-array fill lands where the walk named the array — FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::whole_array_write_target` —
which resolved the operand with `Shell::resolve_ref_array_write` (correctly) and
then **threw the scope away**, returning a bare name. Its two callers,
`Shell::builtin_read`'s `-a` arm and `Shell::builtin_mapfile`, filled whatever
that name meant *live*, so a chain that escaped a cycle to global scope filled
the local reference instead of the global — and, filling it, set `-a` on top of
`-n`, a shape bash never produces:

```sh
g1=(A B); f1() { local -n g1=g1; read -a g1 <<< "P Q"; declare -p g1; }
f1 2>/dev/null; declare -p g1
# bash: declare -n g1="g1"                 osh was: declare -an g1=([0]="P" [1]="Q")
#       declare -a g1=([0]="P" [1]="Q")             declare -a g1=([0]="A" [1]="B")
```

**The rule.** `builtin_find_indexed_array` (builtins/common.c:1029) opens with
`find_or_make_array_variable` (arrayfunc.c:465), whose first line is a plain
`find_variable (name)` — the *read* walk, which takes a local cycle's
`find_global_variable_noref` escape — and which then holds the `SHELL_VAR *` for
the whole fill. So every question the builtin asks (readonly, noassign, "not an
indexed array") is asked of **that** variable, the value attributes applied are
that variable's, and the elements land there. The one thing that is *not* asked
that way is a `mapfile -C` callback: it is ordinary shell code, so inside it the
name still means the local reference.

**The fix.** `whole_array_write_target` now answers `(name, RefScope)` and asks
its three questions under `Shell::in_scope`. `read -a` takes one swap around the
whole fill; `mapfile` takes a swap around each store — the flush, each element
(with its value attributes) and `after_var_write` — and deliberately leaves the
callback outside, which is what makes `cb` above read the global one element
behind the fill rather than the element being stored.

**Corpus:** `a-whole-array-fill-lands-where-the-walk-named-the-array.sh`.

**How it was found:** probing which assignment forms go through `bind_variable`
while implementing the per-context write walk (see
TD-OILS-A-WRITE-THROUGH-A-CHAIN-OF-LOCALS-GIVES-UP-ONE-LINK-EARLIER). `read -a`
was set aside then as belonging to the cycle-escape entry, and re-measured
afterwards — at which point it turned out to be neither, but a scope dropped on
the floor between the resolver and the fill.
