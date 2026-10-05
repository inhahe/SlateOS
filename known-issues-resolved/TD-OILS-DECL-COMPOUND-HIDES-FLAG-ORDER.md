### TD-OILS-DECL-COMPOUND-HIDES-FLAG-ORDER. A flag word written *after* a compound operand is still read as a flag — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `exec_declare_with_arrays_scoped`'s
flag loop and its phase-2 call, both of which run over `argv`. Compound
`name=(…)` operands were lifted out of `argv` into `decl_arrays` before
either sees it, so a flag word that in the source sat *behind* an operand
looks to them like a leading flag.

bash parses a declaration builtin's flags with getopt, which stops at the
first non-option word; everything from there on is an operand, and an
operand that is not an identifier is refused:

```sh
declare k1=1  -p    # bash and osh agree: `-p': not a valid identifier, rc 1
declare k4=(1) -x   # bash: `-x': not a valid identifier, rc 1
                    # osh:  binds k4 and exports it, rc 0
declare k5=(1) -a   # bash: `-a': not a valid identifier, rc 1;  osh: rc 0
```

The scalar spelling already matches — `builtin_declare` does stop at its
first operand. Only the compound spelling diverges, because the operand
that should have stopped the scan is not in the list being scanned.

Reproduce with `target/dvscratch/px50.sh` (the "compound case" section is
the whole diff) and the `-p after the operand` line of `px46.sh`.

**Fixed** by carrying the boundary alongside the spliced word list.
`exec_simple_inner` knows where each compound operand sat
(`word_starts[d.word_index]`), so it computes the argv index of the
earliest one and hands it down as `DeclWords::flag_limit`. The scoped
function's own flag loop stops there, and so do
`builtin_declare_scoped`/`builtin_readonly`/`builtin_export`, which then
see the trailing words as operands — and their existing operand loops
already answer a non-identifier with the right message, under the right
tag, without stopping. Covered by
`tests/corpus/declare-flag-after-operand.sh` and the unit test
`a_flag_written_behind_a_compound_operand_is_an_operand`.

One wart remains, and it is TD-OILS-DECL-REFUSAL-ORDER's rather than this
entry's: a forced-operand word is diagnosed by phase 2, so it still
precedes any phase-3 refusal of a compound operand written *before* it.
