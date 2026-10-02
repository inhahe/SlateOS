### TD-OILS-COND-UNARY-TABLE-DRIFT. `[[ ]]` knew 13 unary primaries where `test` knew 26 — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** two independent tables and two independent evaluators for what bash
treats as one thing —

| | table | evaluator |
|---|---|---|
| `[[ … ]]` | `parser.rs`'s `unary_op_from` (14 entries, 13 distinct tests) | `interp.rs`'s `cond_unary`, dispatching on an `ast::UnaryOp` enum |
| `test` / `[` | `interp.rs`'s `is_test_unary_op` (26 entries) | `interp.rs`'s `eval_test_unary` → `eval_unary` |

**What.** bash draws both spellings from one list, so all 26 primaries parse in
both and answer the same for the same operand. osh's `[[ ]]` list was missing
13 of them, and a missing one is not a false answer but a **syntax error**:

```sh
v=x; declare -n nr=v
[[ -R nr ]]   # bash: true    osh: conditional binary operator expected (status 2)
[ -R nr ]     # bash: true    osh: true          ← the same test, one spelling apart
```

Missing entirely from `[[ ]]`: `-a -b -c -g -k -p -u -G -N -O -R -S`. Worse than
absent, `-r`/`-w`/`-x` were *present but different* — `cond_unary` approximated
them (`-r` ≈ exists, `-x` ≈ exists) while `eval_unary` had the real
`access(2)`-shaped `unary_access`, so the two spellings could disagree on a file
that exists but is not executable.

**Root cause is the shape, not the omission.** Nothing tied the two tables
together, so `[[ ]]` was frozen at whatever set was implemented first while the
builtin's grew. This is the "two things that must stay in sync" failure: the
`UnaryOp` enum bought nothing — the spelling had to be kept anyway for `set -x`
traces and `declare -f` reprints — and cost a second table plus a second
evaluator.

**Fixed in the commit that added this note** by deleting the duplicate rather
than filling it in. `ast::unary_op_text` is now the single table (26 spellings,
returning the matched `&'static str`); `unary_op_from` and `is_test_unary_op`
are both one call to it, `CondUnary` carries only `text`, `ast::UnaryOp` is
gone, and `cond_unary` is an operand expansion, a trace, and a call to
`eval_test_unary` — the builtin's own evaluator. The two surfaces can no longer
diverge because there is only one of each.

Measured against bash 5.2.37: exactly those 26 parse in `[[ ]]` and nothing else
does (`-q`, `-Q`, `-1` are all `conditional binary operator expected`), and
`[[ op X ]]` matches `[ op X ]` for every primary over the operands
`f`/`d`/`nope`/empty. `-a` is the primary only in leading position — `[[ x -a y ]]`
stays a syntax error, since `[[ ]]` has no `and` connective spelled that way.

Covered by the corpus case
`a-conditional-and-test-share-one-set-of-primaries.sh`, the lib test
`a_conditional_knows_every_test_primary`, and the parser test `cond_operators`.
