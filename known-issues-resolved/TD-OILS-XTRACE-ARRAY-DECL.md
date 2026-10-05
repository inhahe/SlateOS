### TD-OILS-XTRACE-ARRAY-DECL. `declare -a b=(x "")` traces the `declare` without its arguments, and does not trace the compound value — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — the `set -x` trace block in
`exec_simple`, and the `declare`/`local`/`readonly` argument path.

**Reproduce.**

```
$ bash -c 'set -x; declare -a b=(x "")'
+ b=('x' '')
+ declare -a b
$ osh -c 'set -x; declare -a b=(x "")'
+ declare -a
```

Two separate faults in one line: the `b=(…)` word is dropped from the traced
`declare` command, and bash's *extra* line for the compound value — traced in the
`xtrace_print_assignment` style, each element minimally quoted — is not emitted
at all.

**Contrast:** a plain `arr=(1 "" 3)` (no `declare`) already traces correctly, as
the verbatim source text, in both shells. So this is specific to a compound
assignment appearing as an *argument* to a declaration builtin.

**Resolution (2026-07-30).** Both faults fixed; pinned by
`xtrace_traces_a_declaration_builtins_array_operands` and the corpus case
`tests/corpus/xtrace-array-decl.sh` (byte-identical to bash over 40 shapes).

The measured rule, which the implementation follows exactly:

- One `name=(…)` line per compound operand, in operand order, **all before** the
  builtin's own line. bash performs such an assignment during the command's
  *word-expansion* pass, not inside the builtin, so this ordering is not cosmetic —
  it is the same phase split that `exec_declare_with_arrays`'s three phases encode.
- The elements shown are the fully expanded **fields** (split, globbed,
  brace-expanded), each wrapped in single quotes **unconditionally**. Confirmed:
  the compound line does *not* use the four-branch `xtrace_print_word_list`
  cascade, so a plain `x` prints as `'x'` and a control character stays raw
  (`c=('a\002b')`, never `$'…'`). Hence a separate `xtrace_compound_quote` rather
  than a reuse of `xtrace_quote_value`.
- A keyed element renders `['idx']='val'`, the subscript being its
  **post-word-expansion, pre-arithmetic, untrimmed** text — `['1+1']`, `[' 1 ']`,
  and `['2']` for `[$i]` with `i=2`. In associative **pair** mode a keyed element
  instead renders as the reassembled `'[a]=1'` word that pair mode treats it as.
- An empty compound traces `name=()`; `+=` is preserved; a command substitution
  emits its `++` line first and exactly once; a **failed** expansion emits no line
  at all.
- The builtin's own line shows a **bare name** where the operand was written, at
  the operand's original position among the words: `+ declare -x SC=1 arr SD=2`,
  `+ declare -a p q r s t`.

Implementation notes:

- None of what the line needs survives the binding (the stored array has evaluated
  subscripts, `-i`-evaluated and case-folded values, and no record of which
  elements were keyed), and re-expanding the literal would re-run its command
  substitutions. So the rendering is recorded *during* the expansion pass, through
  a new `Shell::xtrace_compound: Option<Vec<String>>` side-channel armed by
  `exec_declare_with_arrays` around each `apply_assignment` — the same idiom as the
  existing `discard_error`/`glob_error` flags. `xtrace_compound_elem` pushes an
  element, `flush_xtrace_compound` emits the line at each branch's genuine
  "expansion complete" point, and it is one-shot so pair mode's earlier flush wins.
- The operands' *positions* had to be recorded in the AST: the new
  `ast::DeclArray { assign, word_index }` replaces the bare `Assignment` in
  `SimpleCommand::decl_arrays`, since the operands live outside `words` but their
  source order is observable both here and in `$BASH_COMMAND`. `unparse.rs`'s
  `simple_inline` now splices them back at `word_index` too, which fixes
  `$BASH_COMMAND` for `declare -x SC=1 arr=(9) SD=2` as a side effect.
- `eval_arith_index` was split so the keyed path can trace the subscript's text
  before evaluating it (`eval_arith_index_text`).

Two divergences remain in this area, both from the same interleaving in the
associative branch and both tracked separately:
TD-OILS-DECL-COMPOUND-NO-BIND-ON-FAIL and TD-OILS-DECL-COMPOUND-DISCARD-LEAK. One
bash display bug is deliberately not replicated: TD-OILS-XTRACE-CTLESC-LEAK.
