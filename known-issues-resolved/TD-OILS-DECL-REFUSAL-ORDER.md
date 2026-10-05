### TD-OILS-DECL-REFUSAL-ORDER. A refused *compound* operand's diagnostic is printed after a refused scalar one, whatever order they were written in — 2026-07-31 — ✅ RESOLVED 2026-08-03

**Where:** `userspace/oils/src/interp.rs` —
`exec_declare_with_arrays_scoped`. bash has one loop over all of a
declaration builtin's name operands, so its per-name diagnostics come out
in operand order. osh splits them: compound operands bind in phase 1,
phase 2 hands the *scalar* operands to `builtin_declare_scoped` (which
emits their refusals), and phase 3 walks the compound operands again to
apply the attributes the builtin could not see — and emits *their*
refusals. So a compound operand's refusal always trails a scalar one.

```sh
declare -aA m11=(1) m12=zz
# bash: declare: m11: …   osh: declare: m12: …
#       declare: m12: …        declare: m11: …
```

Both diagnostics are right, both operands end up in the right state, and
the status is 1 either way — only the two stderr lines are transposed.
The same wart applies to the `cannot destroy array variables in this way`
refusal, which has been in phase 3 much longer; it is simply invisible
unless the *scalar* operand also fails.

**Proper fix.** `ast::DeclArray` already carries `word_index`, so the
original operand order is recoverable. Hand the compound operands to
`builtin_declare_scoped` as pre-bound entries spliced into its operand
list at their `word_index`, and let its single loop do the refusals and
the attribute application for both kinds. That also removes the phase-3
loop, and with it the "phase 3 must come after the builtin so `readonly`
does not reject the initializer" constraint — a compound's value is bound
in phase 1 regardless. `readonly`/`export` route to their own builtins
and would keep a small phase-3 remnant.

Reproduce with `target/dvscratch/px40.sh` (the last section) — it is the
only line of that probe that still differs.

The `spliced` word list added for `-p` on 2026-07-31 (see
TD-OILS-DECL-COMPOUND-HIDES-FLAG-ORDER below) is exactly the list this
fix wants: `argv` with each compound operand's bare name put back at its
source position. It is already threaded from `exec_simple_inner` down
through `exec_declare_with_arrays` to `exec_declare_with_arrays_scoped`.

**Fixed 2026-08-03, as the proper fix above.** Phase 1 now records what it
leaves for the builtin in a `BoundCompound` per operand — where the
binding landed, and the two refusals only it can decide (a `noassign`/
readonly-global local, and `-n` against a name that is not already a
reference) — each stamped with the position the operand held in `argv`.
`builtin_declare_scoped` takes that list alongside its own words, merges
the two into one operand sequence in source order, and calls the new
`Shell::apply_bound_compound` when it reaches a compound one, so a
compound and a scalar refusal interleave exactly as bash's single loop
prints them. The phase-3 loop is gone for the `declare` family; what is
left of it applies the attributes for `readonly`/`export` only, whose
scalar operands went to entry points of their own — and none of the three
refusals can fire there (neither builtin makes locals, reads `-n` as the
nameref letter, or reads a `+` word as a flag at all, and the array-kind
conflict is the `declare` family's alone).

Two side-effects worth noting. The held local refusal is now spoken *by
the builtin*, i.e. only when the builtin actually reaches the operand, so
`local -Z ro=(1)` — an invalid option, which bash's getopt turns away
before it looks at any operand — no longer prints the builtin's half and
now returns 2 rather than 1, which is what bash does. And `declare -p`,
which returns before any operand is applied, keeps printing that half on
its own, since a printing command still *reaches* the builtin.

Covered by `declaration_refusals_come_out_in_operand_order` and
`userspace/oils/tests/corpus/a-declarations-refusals-come-out-in-operand-order.sh`,
which was confirmed to fail on the parent commit and pass on this one.
