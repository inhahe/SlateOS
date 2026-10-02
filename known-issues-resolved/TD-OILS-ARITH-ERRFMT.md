### TD-OILS-ARITH-ERRFMT. Arithmetic error messages don't match bash's `<expr>: <msg> (error token is "<tok>")` format/taxonomy — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/arith.rs` — `ArithError { msg, token }` payloads
raised throughout `parse`/`parse_*`/`parse_number`/`str_to_val`, and the
`emit_arith_error` / `eval_arith_cmd` call sites in `interp.rs` that surface them.

**Resolution (2026-07-19):** `osh` now reproduces bash's full arithmetic
diagnostic line `<name>: line N: [<builtin>: ]<expr>: <body> (error token is
"<tok>")`:

- **`ArithError` carries a token.** Was `ArithError(String)`; now
  `ArithError { msg: String, token: Option<String> }` with `new()` /
  `with_token()` constructors and a `Display` impl that appends
  ` (error token is "<tok>")` when a token is present. The `AParser` tracks
  `last_op_start` / `last_atom_start` and a `rest_from(pos)` helper so every
  raise site can emit "offending-position-to-end-of-input, de-quoted" as the
  token (operand-expected → operator/last-op position; trailing-input → current
  position; ternary `:` → then-branch start; assignment → operator position;
  bad subscript → array-name start; missing `)` → last-atom start; div/mod/exp →
  RHS operand start, threaded via `Expr::Bin`'s 4th field).
- **Body wording matches bash's taxonomy:** `division by 0`, `exponent less
  than 0`, `syntax error: operand expected`, `syntax error in expression`
  (recognized trailing token) vs `syntax error: invalid arithmetic operator`
  (untokenizable char), `` `:' expected for conditional expression ``,
  `bad array subscript`, `attempted assignment to non-variable`, `` missing `)' ``,
  `invalid arithmetic base`, `value too great for base`,
  `expression recursion level exceeded`.
- **`<expr>:` prefix + builtin tag.** `interp.rs::emit_arith_error` prints the
  (leading-whitespace-trimmed) source expression, and `arith_cmd:
  Option<&'static str>` models bash's `this_command_name` so the right builtin
  tag is prepended: `let:` for `let`, `((:` for `(( ))` and `for (( ))`,
  `declare:`/`typeset:`/`local:` for the `-i` attribute builtins. Plain
  assignments, array-element assignments, and `$(( ))` word substitution get no
  tag (matching bash).
- **Recursively-expanded `<expr>` prefix (FIXED 2026-07-20).** When a failure
  occurs while recursively evaluating a *variable's value* as arithmetic, bash
  echoes the resolved value, not the variable reference — `x="5 apples"; $((x))`
  reports `5 apples: syntax error …`, not `x: …`. `arith.rs::str_to_val` now
  records the innermost failing value in `ArithError::expr_override` (the deepest
  level sets it first as the error unwinds; outer levels leave it in place), and
  `emit_arith_error` prefers it over the top-level source. This also covers the
  `expression recursion level exceeded` case (bash echoes the innermost value)
  and indirection chains (`x=y; y="1 2"; $((x))` → `1 2:`). Verified against
  MSYS bash for `x="5 apples"`, `x=3.5`, `x="1 2"`, `x=y;y=…` and the `declare
  -i z=x` builtin-tag path.

**Tests:** `arith.rs::error_bodies_and_tokens_match_bash` (16 body/token cases)
and `interp.rs::arith_error_matches_bash_format` (full-line, incl. builtin tags
and `2>/dev/null` silencing). Verified byte-for-byte against MSYS bash on 25/27
probed cases (name-normalized).

**Residual divergences (documented bash yacc artifacts, low value — left as-is):**

- **Exponent error token:** `$((2**-1))` → bash reports `1` (its lexer's
  last-consumed token), osh reports `-1` (the RHS operand source). bash is
  internally inconsistent here (division uses the whole RHS source; exponent
  uses the last lexed token) — osh picks the single consistent
  offending-position rule.
- **Nested subscript prefix:** `$((a[9/0]))` → bash echoes `9/0` as the expr,
  osh echoes `a[9/0]` (the full atom). A yacc reduction artifact.
- **Function-scope name:** in a function body bash's `<name>` becomes
  `environment`; osh keeps `$0` (= `osh`) per design §74.
