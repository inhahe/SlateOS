### TD-OILS-A-RUN-OF-PARENS-IN-A-CONDITIONAL-IS-LEXED-AS-ARITHMETIC. `[[ ((( a ))) ]]` is a syntax error where bash succeeds — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/lexer.rs` — the tokenizer's `'('` arm, where a
flush `((` becomes `Tok::ArithCmd`. The gate has to be bash's, which is the
*same* `reserved_word_acceptable (last_read_token)` this file's other
conditional-paren entry turns on.

**What.** bash only tries the arithmetic reading of `((` when `read_token` is at
a position where a reserved word would be acceptable: `parse_dparen`
(parse.y:4457) runs `parse_arith_cmd` under `if (reserved_word_acceptable
(last_read_token))` and otherwise returns `-2`, which makes `read_token` `break`
out of the `shellmeta` switch and hand back a plain `(`. `COND_START` — the token
`[[` — is **not** in `reserved_word_acceptable`'s list (parse.y:5367), but `'('`
**is**. So the first `(` after `[[` is always an ordinary group opener, and only
a `((` *nested inside* a group takes the arithmetic path.

osh lexes a flush `((` as an arithmetic command wherever it sees one, so a run of
three or more parens is mis-split:

```text
input                    bash    osh
[[ ((( a ))) ]]          ok      line 2: expected `)'
                                 line 2: syntax error near `)))'
                                 line 2: `[[ ((( a ))) ]]'
[[ (((( a )))) ]]        ok      … syntax error near `))))'
[[ ( ( ( a ))) ]]        ok      ok      (open run broken up — no `((`)
[[ ((( a ) ) ) ]]        ok      ok
[[ (( a )) ]]            ok      ok
[[ (( ]]                 both: two `expected `)'` + `syntax error near `]]''
```

The unterminated form shows the same split from the other side, and adds a raw
end-of-input sentinel leaking into a message as `\xff`:

```text
echo 1⏎[[ (((⏎    bash:  line 3: unexpected token `EOF' in conditional command
                         line 2: expected `)'      (three times)
                         line 3: syntax error: unexpected end of file
                  osh:   line 2: unexpected EOF while looking for matching `)'
                         line 3: unexpected token `M-^?' in conditional command
                         line 2: expected `)'
```

**Fixed.** The lexer's `((` test now also requires `self.cond_depth == 0`, so
inside a `[[ … ]]` every paren opens a group and nothing else.

The analysis written above was half wrong, and the measurement corrected it.
`'('` *is* on `reserved_word_acceptable`'s list, so the reasoning "only a `((`
nested inside a group takes the arithmetic path" looks right — but it predicts
that `[[ (( 0 )) ]]` evaluates `0` arithmetically and is therefore false. bash
returns 0 (true) for it, and 1 for `[[ (( "" )) ]]`: those are *string* tests, so
no paren inside a conditional is arithmetic, not merely the first.

The reason is that `last_read_token` is assigned in exactly one place, `yylex`
(parse.y:2902–2905), and a conditional never goes through it. `parse_cond_command`
runs from *inside* a single `read_token` call (parse.y:3395–3408) and `cond_term`
asks `read_token` for its tokens directly (parse.y:4625/4647). So for the whole
length of a `[[ … ]]`, `last_read_token` is frozen at `COND_START` — which is not
on the list — and `parse_dparen` (parse.y:4457–4511) returns `-2` for every `((`
in there.

The `\xff` leak went with it: the sentinel was only ever reached because a
conditional had been mis-split into an arithmetic command. `[[ (((` now reports
`unexpected token `EOF' in conditional command`, as bash does.

**Pinned by** `lexer.rs`'s `no_paren_inside_a_conditional_is_arithmetic` (group
depths 1–5, the `(( 0 ))` / `(( "" ))` discriminator, the exact token shape of
`[[ ((( a ))) ]]`, and the untouched `((1))` / `for ((…))` / `x=1 ((2))` /
`echo ((1))` cases) and the corpus case
`no-paren-inside-a-conditional-is-arithmetic.sh`.

**Impact.** A conditional written with three or more nested groups and no space
between the parens failed to parse. Rare in practice, but it was a *valid*
program rejected, not just a differently-worded error.
