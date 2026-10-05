### TD-OILS18. `osh` `declare -f` / `type funcname`: function *body* is now printed — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`declare_functions`, the `type`
builtin's function branch, and `builtin_set`'s no-args listing — TD-OILS16);
`userspace/oils/src/unparse.rs` (the pretty-printer).

**What:** `declare -F` fully lists/tests function names, and `declare -f name`
reports the correct existence status, but neither `declare -f` nor
`type funcname` printed the function *body* — they needed a faithful
`FunctionDef`-AST → shell-source pretty-printer, which the AST could not do
directly (it carries no source spans, so the text must be reconstructed from the
parsed tree).

**Resolution:** added `unparse.rs`, an AST-to-source pretty-printer covering
`Program`/`Command` (all compound forms), `Word`/`WordPart` (every parameter-
expansion variant, command/arith substitution, arrays/slices/bulk ops),
assignments, redirections, and `[[ … ]]`/`(( … ))` — verified by a round-trip
stability property test (`parse(print(f))` re-prints identically; the AST derives
`PartialEq`). It is now used by: bare `declare -f` and `declare -f NAME` (print
each function's reconstructed source), `type NAME` (prints the "is a function"
line then the source), and bare `set` (lists functions after variables, closing
TD-OILS16). ~~One deliberate simplification: here-documents are re-emitted as
here-strings (`<<< …`) — same bytes to stdin, re-parseable.~~ **Superseded
2026-07-28:** the here-string simplification was wrong for any body with more
than one line (`cat <<EOF` / `line one` / `line two` / `EOF` printed as
`cat <<< line one` followed by a bare `line two` line, which re-parses as a
second command). `ast::Redirect` now carries the delimiter, its quoting and the
`<<-` flag, and `unparse.rs` prints a real here-document; see the here-document
follow-on under TD-OILS-DECLAREF-QUIRKS for the placement quirks of bash's own
printer that are deliberately not reproduced. Regression tests:
`declare_small_f_prints_body`, `type_function_prints_body`,
`bare_set_lists_functions`, and the `unparse::tests` round-trip suite.
