### TD-OILS-DECL-COMPOUND-NO-BIND-ON-FAIL. A failed operand expansion should bind nothing and create nothing — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `exec_declare_with_arrays` phase 1
(the `array_kind_apply` / attribute block that runs *before* `apply_assignment`),
and the associative keyed-mode branch of `apply_assignment` (~7414).

**What.** Two divergences, both from the same cause: osh applies the declared kind
and attributes — and, in keyed mode, binds each element as it is expanded — before
knowing whether the operand's expansion will succeed. bash performs the whole
compound assignment inside the command's word-expansion pass, so a failure there
aborts the command before anything is created or bound. Measured:

```sh
declare -a fg=(x $((1/0))); declare -p fg
# bash: declare: fg: not found      osh: declare -a fg   (created, empty)

declare -A k3; k3=([z]=old)
declare -A k3+=([pk]=pv [j$((a+))]=x); declare -p k3
# bash: declare -A k3=([z]="old" )  osh: ... [pk]="pv" also bound
```

The incremental binding is *correct* for a **bare** compound assignment —
`k1=([z]=old); k1+=([pk]=pv [j$((a+))]=x)` does leave `k1[pk]=pv` set in bash, in
both pair and subscript mode — so the two cases must stay distinguishable.
`self.decl_builtin_ctx` already marks which one is running.

Also from the same interleaving: the `must use subscript when assigning
associative array` diagnostic is emitted mid-expansion, where bash emits it during
the later processing pass. Observable order differences:

```sh
declare -A z1=([a]=1 bad [b]=$(echo oops >&2))
# bash: oops, then the diagnostic.  osh: the diagnostic, then oops.
declare -A z2=([a]=1 bad [b]=$((1/0)))
# bash: only the div-by-zero.       osh: the diagnostic as well.
```

Under `set -x` this makes osh print the diagnostic *before* the operand's trace
line where bash prints it after (`tests/corpus/xtrace-array-decl.sh` deliberately
avoids the shape).

**Proper fix.** Restructure the associative branch of `apply_assignment` into a
single expand-all-then-process shape (the indexed branch already has it): expand
every element into a `Vec<(Option<key>, value)>`, remembering the index at which
expansion failed, then

* failure + `decl_builtin_ctx` → bind nothing, emit no per-element diagnostics,
  return false (bash aborts the command during word expansion);
* failure + bare compound → process the successfully-expanded prefix (so an
  append keeps it), then return false;
* no failure → flush the trace line, then process every element, emitting the
  `must use subscript` diagnostics in element order as part of processing.

Separately, phase 1 of `exec_declare_with_arrays` must not leave the variable
created when the operand fails. The kind has to be in force *before* the elements
bind (it selects the assoc vs. indexed branch), so either thread the intended kind
and value attributes into `apply_assignment` and let it apply them once expansion
has succeeded, or snapshot and restore the name's storage and attribute-set
membership around the call.

**Resolution.** Both halves, exactly as planned, plus one thing the plan missed.

*The associative branch* is now three explicit shapes. Subscript mode under
`decl_builtin_ctx` expands the whole element list into a
`Vec<(Option<key>, value)>` and only then processes it through the new
`assoc_keyed_element` helper, so a failure binds nothing and diagnoses nothing;
the bare form keeps its interleaved expand-then-bind loop, because that
interleaving is observable (`d+=([a]=1 loose [b]=$((1/0)))` keeps `d[a]` and
reports `loose` *before* the division); pair mode gained a `decl_builtin_ctx`
guard that returns early instead of binding the flattened prefix.

*Phase 1* takes the snapshot/restore route — `snapshot_var` immediately after the
`local` shadow (so a restore undoes only what this operand did), `restore_var` on
the failure path. The route mattered: the alternative of deferring the kind and
attributes into `apply_assignment` cannot express bash's behaviour on a
**readonly** target, which *does* get them (`readonly rs=1; declare -a rs=(2)`
leaves `declare -ar rs=([0]="1")`).

*The thing the plan missed* is that "the operand failed" is two regimes, not one,
and only the first rolls back. A failure in the *word-expansion* pass leaves the
name untouched; a failure in the *binding* pass keeps whatever bound first,
attributes included, because the variable really has been written by then:

```sh
declare -ai b=([0]=1 [1]=2+ [2]=3); declare -p b
# bash and osh: declare -ai b=([0]="1")
```

`discard_error` alone cannot tell them apart (a bad `-i` element value arms the
same flag as a bad expansion), so `Shell::compound_expanded` now records the
boundary. It is set by `compound_expansion_done` — the renamed
`flush_xtrace_compound`, which was already called at exactly the
expansion-complete instant in every branch, and whose two jobs are genuinely the
same event — and read by `exec_declare_with_arrays` around each operand.

Covered by `a_failed_operand_expansion_leaves_its_name_exactly_as_it_was` and the
tail of `tests/corpus/declare-compound-operands.sh`. The `must use subscript`
ordering noted above came out right as a side effect: the diagnostics are now part
of the processing pass, so they follow the operand's `set -x` line.
