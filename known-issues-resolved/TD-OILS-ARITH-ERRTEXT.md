### TD-OILS-ARITH-ERRTEXT. `osh` arithmetic error *messages* don't match bash's `<expr> : <msg> (error token is "<tok>")` format — RESOLVED 2026-07-20 (superseded by TD-OILS-ARITH-ERRFMT)

**Known residual (cosmetic, minor):** for an eval-time error whose offending
operand carries a **unary prefix** (`-`/`+`/`~`/`!`), osh's `error token` includes
that prefix while bash's token pointer skips it. Example: `$(( 2 ** -1 ))` →
bash `exponent less than 0 (error token is "1 ")`, osh `… (error token is
"-1 ")`. The message body and exit status match; only the parenthetical token
text differs. osh attaches the whole RHS operand span (`arith.rs` `eval_expr`,
`rhs_tok`) as the token, whereas bash reports the source from the innermost
primary. Fixing it means recording the post-unary-prefix source position on the
operand node — the same `Expr`-span refactor the proper fix below describes — for
a token that essentially no script inspects. Deferred with the rest of this item.

**Resolution:** superseded by the completed TD-OILS-ARITH-ERRFMT work. All three
"concrete diffs" listed below now reproduce bash 5.2 exactly, verified 2026-07-20:
`$((5/0))` → `5/0 : division by 0 (error token is "0 ")`; `$((09))` → `09: value
too great for base (error token is "09")`; `$((3.5))` → `3.5 : syntax error:
invalid arithmetic operator (error token is ".5 ")`. The historical analysis
below is retained for context only.



**Where:** `userspace/oils/src/arith.rs` — every `Err(ArithError(...))` site
(`parse`, `parse_atom`, `parse_number`, `apply`, `eval_expr`) and the top-level
`eval`. `ArithError` is a bare `String` with no position/token info.

**What:** on an arithmetic error, bash prints a very specific format that osh
does not reproduce. Two patterns (measured against bash 5.2):

- **Pattern A** (parse/eval errors): `<expr> : <message> (error token is "<tok>")`,
  where `<expr>` is the whole arithmetic string (leading whitespace stripped),
  there is a space *before* the colon, and `<tok>` is the **remaining unparsed
  input from the error position to the end** (double-quotes already removed, and
  it keeps trailing whitespace). Messages seen: `division by 0` (both `/` and
  `%` by zero), `exponent less than 0`, `syntax error: operand expected`
  (`1+`, `x=`, `1&&&2`), `syntax error in expression` (`a b`, `1 2` — an operand
  where an operator was expected), `syntax error: invalid arithmetic operator`
  (`3.5` — a non-operator char where an operator was expected).
- **Pattern B** (numeric-base errors): `<token>: <message> (error token is "<token>")`
  — the prefix is the offending *number token itself* (no leading space, no
  space before the colon), for `value too great for base` (`09`, `0xZ`) and
  `invalid arithmetic base` (`99#5`).

Concrete diffs:
```
$(( 5/0 ))   bash: 5/0 : division by 0 (error token is "0 ")
             osh:  arithmetic: division by zero
$(( 09 ))    bash: 09: value too great for base (error token is "09")
             osh:  arithmetic: bad octal literal '09'
$(( 3.5 ))   bash: 3.5 : syntax error: invalid arithmetic operator (error token is ".5 ")
             osh:  arithmetic: unexpected trailing input in arithmetic: '3.5'
```

**Why deferred:** the parse-time errors can take the "error token" from the
parser's `pos` (`chars[errpos..]`), but the **eval-time** errors (division by
zero, modulo by zero, exponent < 0) fire during `eval_expr`/`apply` on the AST,
*after* parsing — there is no parser position available, so matching bash's
token there requires annotating every `Expr` node with a source span (byte
range) and threading it through construction and evaluation. That is a real
refactor of the `Expr` enum for purely cosmetic stderr text that essentially no
script parses. Behavioral (result-affecting) divergences are higher value.

**Proper fix:** (1) add a `span: (usize, usize)` to the relevant `Expr` variants
(or a parallel side-table keyed by node), populated at parse time from `pos`;
(2) classify errors into an enum carrying the bash message text and the error
position; (3) in `eval`, format Pattern A as `{expr.trim_start()} : {msg} (error
token is "{chars[pos..]}")` and Pattern B (base errors) as `{tok}: {msg} (error
token is "{tok}")`. Match bash's operand-vs-operator "syntax error" distinction
by inspecting the char at the failure position.
