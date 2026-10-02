### TD-OILS5. `osh` arithmetic: assignment/increment operators + C-style `for (( ; ; ))` — RESOLVED 2026-07-18

**Where:** `userspace/oils/src/arith.rs` (`VarLookup` trait, `eval`, the
`Expr` AST + `AParser`), `userspace/oils/src/interp.rs` (`impl VarLookup
for Shell`, `eval_arith_raw`, `exec_for_arith`), `userspace/oils/src/ast.rs`
(`ForArithClause`), `userspace/oils/src/parser.rs` (`parse_for_arith`).

**Resolution:** `arith.rs` was rewritten from a fused parse+eval into a
two-phase **AST design**: `parse(expr, &dyn VarLookup) -> Expr` (immutable
borrow) then `eval_expr(&Expr, &mut dyn VarLookup)` (mutable borrow), so
`eval` is now `eval(expr, &mut dyn VarLookup) -> i64`. `VarLookup` gained
`set`/`set_index`/`set_assoc` (empty defaults; `impl … for Shell` writes
back to `vars`/`arrays`/`assoc`). The evaluator now supports the full
mutation set: assignment `= += -= *= /= %= <<= >>= &= |= ^=` (right-assoc,
looser than `?:`, tighter than `,`; chained `a = b = c` works), pre/post
increment/decrement `++x`/`x++`/`--x`/`x--`, and exponentiation `**`
(right-assoc). `&&`/`||` short-circuit and `?:` is branch-lazy, so side
effects only fire on the taken path. Array/associative element assignment
(`a[i] = …`, `m[key] += …`) resolves the subscript once (`ResolvedLv`).

The **C-style `for (( init; cond; update ))` loop** is implemented: the
lexer already emits `(( … ))` as a single `ArithCmd` token, so `parse_for`
detects it, splits the raw text on `;` into three sections
(`ForArithClause`), and `exec_for_arith` runs `init` once, loops while
`cond` is non-zero (empty ⇒ always true), and runs `update` after each
iteration (including after `continue`). `break`/`continue` with a level
count propagate as for other loops.

**Tests:** unit tests in `arith.rs` (`assignment_scalars`,
`increment_decrement`, `indexed_assignment_and_incr`,
`short_circuit_side_effects`, `exponent`, updated
`associative_subscripts`/`comma`) + interp integration tests
(`arith_assignment_command`, `arith_increment_command`,
`arith_assignment_array_elements`, `arith_c_style_for_loop`). All 165
oils tests pass; clippy clean; slateos target builds.
