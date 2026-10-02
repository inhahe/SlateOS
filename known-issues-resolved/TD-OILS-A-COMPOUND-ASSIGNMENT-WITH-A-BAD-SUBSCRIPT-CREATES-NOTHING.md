### TD-OILS-A-COMPOUND-ASSIGNMENT-WITH-A-BAD-SUBSCRIPT-CREATES-NOTHING. `z=([0]=A [1x]=B)` leaves `z` unset — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, the array-literal assignment path
(the `AssignRhs::Array` / `decl_arrays` handling). A keyed element whose
subscript fails to evaluate aborts the whole assignment, so the variable is
never created. bash creates it, keeps every element assigned *before* the bad
one, and drops the bad element and everything after it — `assign_compound_array_list`
(arrayfunc.c) has already made the variable by the time it walks the list, and
`array_expand_index`'s failure only breaks the walk.

**Reproduce.**

```sh
declare -a y=([1x]=R);            echo "1 rc=$?"; declare -p y
declare -a z=([0]=A [1x]=B [2]=C); echo "2 rc=$?"; declare -p z
q=([1x]=R);                        echo "4 rc=$?"; declare -p q
```

| row | bash 5.2.37 | osh |
|---|---|---|
| 1 | `declare -a y` | `declare: y: not found` |
| 2 | `declare -a z=([0]="A")` | `declare: z: not found` |
| 4 | `declare -a q=()` | `declare: q: not found` |

The error text and the `rc=1` already match; only the variable's existence and
its surviving elements differ. An *associative* compound assignment does not
enter this at all — its subscripts are keys, so `declare -A k=([a]=A [1x]=B)`
succeeds with both keys in both shells.

Under `set -x` a second half of the same bug shows: bash traces the whole
operand *before* it evaluates any subscript, so
`set -x; declare -a z=([0]=A [1x]=B [2]=C)` prints
`+ z=(['0']='A' ['1x']='B' ['2']='C')` and *then* the error. osh prints the
error alone — the trace is emitted by `compound_expansion_done`, which the bail
jumps over. (The prefix-assignment spelling `w=(…)` traces correctly in both,
because its line is written from the source rather than from the expansion.)

**The fix.** The two phases in `AssignRhs::Array` are right, but the subscript
*arithmetic* is in the wrong one. bash expands the whole word list first
(`expand_words_no_vars`), and only then walks it in
`assign_compound_array_list`, calling `array_expand_index` per element — so a
subscript that will not evaluate is a phase-2 failure, not a phase-1 one. Move
`eval_arith_index_text` out of phase 1 (phase 1 keeps expanding the subscript's
*text*, which is what the trace needs), and on a failing element break the
phase-2 walk rather than unwinding the assignment. Everything phase 2 has
already done then stands: the trace, the array's creation, the clear, and the
elements bound before the failure. Note row 4's `q=([1x]=R)`: the variable is
created *empty*, so the creation is not conditional on any element succeeding.

Found while probing TD-OILS-A-NESTED-BRACKET-IN-AN-ASSIGNMENT-TARGET-IS-NOT-AN-ASSIGNMENT.

**Fixed** as described: `IndexedElem::sub` now carries the subscript's expanded
*text*, phase 1 only expands it (which is what `set -x` traces), and phase 2's
walk calls `eval_arith_index_text_checked` per element and `return false`s on
the element that fails — bash's `jump_to_top_level (DISCARD)` taken from inside
`assign_compound_array_list`.

Row 1's `declare -a y` turned out to be a second, separable rule that the fix
had to model: the array's *visibility*. bash sheds `att_invisible` per element
bound (`bind_array_var_internal`, arrayfunc.c:245) and again when the
assignment completes (`assign_array_var_from_string`, arrayfunc.c:896), and a
`DISCARD` reaches neither — so what survives is whatever the *creation* chose. A
bare `q=(…)` creates through `find_or_make_array_variable` before the walk and
is therefore already visible (`declare -a q=()`), while a `declare`/`local`
operand's name is created invisible and stays so (`declare -a y`). An
already-invisible name keeps its invisibility under either spelling, and the
per-element clear is sticky — `declare -a f; f=(A [1x]=B); unset 'f[0]'` leaves
`declare -a f=()`, so it is recorded in `array_valued` rather than inferred from
the array being non-empty.

Corpus: `a-compound-assignment-binds-up-to-the-subscript-that-fails.sh`.
