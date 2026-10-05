### TD-OILS-DECL-COMPOUND-OPERAND-TRACED-AFTER-THE-PREFIX. `set -x` orders a declaration builtin's compound operands after the command's prefix assignments, where bash puts them first — 2026-07-31 — ✅ RESOLVED 2026-08-03

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_simple_inner`, which
expands (and so traces) a `declare -a x=(1 2)` operand inside
`exec_declare_with_arrays` at *dispatch* time, after
`Shell::prefix_assignments` has run.

**What.** bash expands a declaration builtin's compound operands during the
word-expansion pass, i.e. before it processes the command's temporary
assignments, so their traces come first:

```
$ bash -c 'set -x; A=(1 2) declare -a x=(3 4)'
+ x=('3' '4')
+ A='(1 2)'
+ declare -a x
$ osh -c 'set -x; A=(1 2) declare -a x=(3 4)'
+ A='(1 2)'
+ x=('3' '4')
+ declare -a x
```

The entry originally said "only the trace order differs". Probing while fixing
it showed that was too kind: the operand's *side effects* are out of order too,
and the prefix's own expansion sees the wrong state.

```
$ bash S: A=$(echo pre >&2; echo a) declare -a x=($(echo op >&2; echo o))
op                                    # osh: pre, then op
pre
$ bash S: A=${u=FROMPREFIX} declare -a z=(${u=FROMOP}); declare -p z
declare -a z=([0]="FROMOP")           # osh: FROMPREFIX
$ bash S: y=(old); Y=$(echo "${y[0]}" >&2) declare -a y=(new)
new                                   # osh: old
```

**Proper fix (done).** Split the command in two across its temporary
environment, exactly as bash splits it, rather than trying to hoist only the
expansion out of a binding order that a whole family of already-matched
behaviours depends on (`TD-OILS-DECL-REFUSAL-ORDER`,
`TD-OILS-DECL-COMPOUND-REFUSAL-TAG-IS-THE-FIRST-COMMAND-ONLY`,
`TD-OILS-DECL-FLAG-PRESCAN`). The seam was already there and already
commented — "everything below this line is the *builtin*" — because the same
boundary decides which diagnostics escape the command's own `2>`; it just had
both halves on the same side of `Shell::prefix_assignments`.

* `Shell::declare_compounds_scoped` (was the first half of
  `exec_declare_with_arrays_scoped`) — the flag prescan and phase 1, returning
  `Result<DeclCompounds, Flow>`: the operand bindings, `-p`, and the attribute
  letters phase 3 applies. `Err` carries the flow a failed operand imposes on
  the whole command.
* `Shell::exec_declare_with_arrays_scoped` — now only the builtin: its trace
  line, its stderr push, `-p`, phase 2 and phase 3.
* `Shell::in_declare_global_scope` — the `-g` swap, extracted so *each* half can
  take it. Entering and leaving once per half is the same as once around the
  whole, because leaving parks what the half made of the global back in the
  frame that shadows it, which is exactly where the next enter reads it from.
  And it must be per-half: bash's `-g` does not leak into the prefix, which
  still reads the local (`local x=(loc); X=${x[0]} declare -ga x=(glob)` reads
  `loc`).
* `Shell::exec_simple_inner` — builds `spliced`/`positions`/`flag_limit` and
  calls the first half *before* `prefix_assignments`, and the second at the old
  dispatch point.

`DeclCompounds` is the carrier. `BoundCompoundFlags` gained `Copy` so phase 3
reads the prescan's letters instead of rebuilding them.

**Verified.** 1249 lib tests (new:
`a_compound_operand_binds_before_the_commands_prefix_assignments`), new corpus
case `a-declarations-compound-operand-runs-before-the-prefix.sh`, and ~30
hand-probed scenarios across `-g`/`local`/`readonly`/`export`/`-p`, the kind,
readonly, nameref and destroy refusals, expansion failures, and the
`2>/dev/null` split — all byte-exact.
