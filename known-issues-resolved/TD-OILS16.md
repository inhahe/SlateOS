### TD-OILS16. `osh` bare `set` lists variables but not function definitions — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`builtin_set`, no-args branch);
`userspace/oils/src/unparse.rs` (the AST source pretty-printer).

**What:** bare `set` (no operands) lists every shell variable in sorted,
re-inputtable `name=value` / `name=([i]="v" …)` form. Bash additionally prints
each defined shell function's full source after the variables (e.g.
`foo () { … }`). Our listing omitted functions.

**Resolution:** implemented a faithful AST-to-source pretty-printer,
`unparse.rs` (`unparse_function`, `program_block`, `word_src`, …), that
reconstructs re-parseable shell source from a `Program`/`FunctionDef` (round-trip
tested: dump → re-parse → dump is stable). `builtin_set` now iterates
`self.funcs` in sorted order after the variables and appends each function's
reconstructed source via `unparse_function`. Shared with TD-OILS18 (`declare -f`)
and the `type NAME` function branch. Regression test: `bare_set_lists_functions`.
