### TD-OILS-A-MARKING-BUILTINS-COMPOUND-OPERAND-WAS-WALKED-AS-CHEAPLY-AS-A-DECLARATIONS. The circular-warning count was 3/1 for every non-local compound operand — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::declare_compounds_scoped`].
The count came from an `escapes` model that gave 3/1 at top level and 2/1 inside
a function, with no separate arm for `readonly`/`export`. Since every walk of a
self-closing nameref chain warns exactly once, the count is a direct measure of
the decomposition, and it was wrong in every `!make_local` arm.

The two builtins pay two walks the declare family does not:

* `-G` (from `W_CHKLOCAL`) makes step 1's `declare_find_variable`
  (declare.def:149) try the nameref-following `find_variable` before falling
  back to `find_global_variable` — exactly one walk more than a plain `-g`.
* Step 2's chklocal branch in `do_compound_assignment` is guarded
  `else if (mkglobal && variable_context)` (subst.c:3491). At top level
  `variable_context` is 0, so the ordinary `assign_array_from_string` runs and
  costs one walk; inside a function the branch is taken and costs two. These two
  builtins are therefore the only spelling that warns *more* inside a function
  than outside one.

Measured, over `declare -n g=z; declare -n z=g`:

```
                       -a / -A   no kind letter (incl. -i)
  local                    2            4
  global, declare-family   1            3    (top level and in a function alike)
  global, readonly/export  2            4 at top level, 5 in a function
```

**Fixed** in this commit; the arm is now a `match (kind_letter, in_function)`
under `global_builtin`.

Corpus:
`a-marking-builtins-compound-operand-is-walked-once-more-than-a-declarations.sh`.
Unit test:
`a_marking_builtins_compound_operand_is_walked_once_more_than_a_declarations`.

Note the corpus must not run inside a function of its own, and its `2>&1` must
be on a *group*: bash expands a command's words before `do_redirections` runs,
so a redirect written on the declaration itself never catches its own
expansion's warnings.

One shape is still wrong and is filed as Active:
TD-OILS-A-FRAME-LOCAL-NAMEREF-CYCLE-IS-HIDDEN-FROM-A-MARKING-BUILTINS-FIRST-WALK.

**How it was found:** a 54-row matrix (18 command spellings × 3 contexts)
written after the value/attribute fix, to check that the relocation of phase 3
had not changed how often the chain is traversed. 6 rows disagreed; 5 of the 6
were this formula.
