## 75. osh arithmetic error tokens — single consistent "offending-position-to-end" rule, accepting documented bash yacc-artifact divergences

**Date:** 2026-07-19
**Decided by:** Claude (operator-approved scope) — the operator authorized the
overall bash-compatibility feature (§74's directive "Continue porting a
bash-compatible shell from oils" / "port all of it"); Claude made the specific
error-token rule call below. See known-issues.md TD-OILS-ARITH-ERRFMT.

**Context.** Extending §74's diagnostic-format work to arithmetic errors, osh
now matches bash's full line `<name>: line N: [<builtin>: ]<expr>: <body> (error
token is "<tok>")`. The `<body>` taxonomy and `<expr>:` prefix are
unambiguous, but the `<tok>` ("error token") is not: **bash's own error-token
choice is internally inconsistent.** For division/modulo bash reports the whole
RHS *source* text; for exponent it reports its lexer's last-consumed *token*;
for a nested array subscript it reports a yacc-reduction fragment; at the
recursion limit it reports the innermost value. Byte-matching all of these at
once would require reproducing bash's exact yacc/lexer state, bug-for-bug.

**Decision.** Adopt one consistent rule for every raise site: the error token
is *the de-quoted source text from the offending position to end of input*
(operator position for operand-expected, RHS-operand start for div/mod/exp,
current position for trailing input, etc.). This matches bash byte-for-byte on
the common cases (25/27 probed) and is predictable/explainable. The residual
edge divergences are documented rather than special-cased:

- `$((2**-1))` — bash token `1`, osh `-1` (bash's exponent uses last-lexed token).
- `$((a[9/0]))` — bash echoes `9/0`, osh `a[9/0]` (yacc reduction artifact).
- recursion limit — bash echoes innermost value, osh the top-level expr.

**Rationale.** *Pro:* one code path, self-consistent behavior, no per-operator
lexer-state emulation; matches bash where it matters. *Con:* three rare
edge-case tokens differ from bash's literal output. Given bash is itself
inconsistent on these, chasing bug-for-bug parity is negative-value.

**Alternatives considered.** (a) Per-operator special-casing to mirror bash's
exact token in every case — rejected: high complexity, emulates bash bugs, brittle.
(b) Omit the error token entirely — rejected: loses real debugging value and the
`(error token is "…")` suffix is the most useful part for locating the fault.

**Where it lives.** `userspace/oils/src/arith.rs` — `ArithError { msg, token }`,
`AParser::rest_from`/`last_op_start`/`last_atom_start`, `Expr::Bin`'s RHS-source
4th field; `userspace/oils/src/interp.rs` — `emit_arith_error`, `eval_arith_cmd`,
`arith_cmd` (bash `this_command_name` model). Tests:
`arith.rs::error_bodies_and_tokens_match_bash`,
`interp.rs::arith_error_matches_bash_format`.

**How to reverse.** Token selection is localized to the `with_token(...)` call
sites in `arith.rs`; the rule can be changed per-site or the token dropped
centrally by making `Display` ignore it.
