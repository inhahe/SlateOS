### TD-OILS-AN-UNFINISHED-CONDITIONAL-AT-END-OF-INPUT-IS-NOT-A-MISSING-BRACKET. `[[ a\` reports the wrong message and line — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Parser::parse_cond_primary` (the
bare-word binary-operator position and the `(` group's line) and
`Parser::cond_operand_error`.

**What.** osh has one answer for "the input ended inside a conditional"; bash
has two, and picks by whether the conditional *grammar* still wanted a token:

```text
echo 1⏎[[ a\⏎    bash: line 4: unexpected token `EOF', conditional binary operator expected
                       line 4: syntax error: unexpected end of file
                 osh:  line 2: unexpected EOF while looking for `]]'
                       line 4: syntax error: unexpected end of file
```

The second line already agrees. The first does not: bash treats the end of file
as an ordinary *token* handed to `cond_term`, which rejects it the same way it
rejects any other wrong token there (`[[ a b ]]` → `conditional binary operator
expected`), and blames it on the reader's line. osh short-circuits to
``unexpected EOF while looking for `]]'`` — which *is* bash's message, but only
for a conditional that is complete and merely unclosed (`[[ a == b⏎` → bash says
exactly that, on the opening line, and osh agrees).

Without the continuation the two agree as well (`[[ a⏎` → ``unexpected token
`newline', conditional binary operator expected`` from both), so what is missing
is only the EOF-as-a-token case.

**Fixed.** The end of input is now a token the conditional grammar rejects where
it stands, in three edits to `parser.rs`:

* The bare-word **binary-operator position** in `parse_cond_primary` no longer
  falls through to `CondExpr::Word` when there is nothing left to read; it
  returns ``unexpected token `EOF', conditional binary operator expected`` — the
  same shape it gives any other wrong token there.
* `cond_operand_error`'s "nothing to peek at" branch names the token `EOF` and
  picks the message from `CondPos`, so an empty **primary**, **unary** or
  **binary** operand slot each say what bash's `error_token_from_token` spells
  for them (``unexpected token `EOF' in conditional command``, ``unexpected
  argument `EOF' to conditional unary/binary operator``).
* A group's `expected `)'` is stamped with `reader_line()` rather than
  `cur_line()`. bash's `cond_term` captures `lineno = line_number` *after* it has
  read the `(` (parse.y:4648), and `(` is a `shellmeta`, so `read_token` peeks
  the next character with `shell_getc (1)` — continuation removal **on** — to see
  whether this is `((`. That peek deletes a `\<newline>` written flush against
  the paren, `line_number++` and all, before ungetting. So `[[ (\⏎` is numbered a
  line further down than `[[ ( a\⏎`, and `[[ ( (\⏎` reports 3 then 2. This one
  change fixed both group rows and `[[ ( a\⏎` along with them.

``unexpected EOF while looking for `]]'`` is kept for the case it belongs to: a
conditional whose operands are all present and whose `]]` is merely missing.

None of the end-of-file forms carries a `syntax error near` line — the fetch that
discovered the end left `shell_input_line` empty, so `report_syntax_error` falls
past both of its `near` branches (parse.y:6273).

**Pinned by** `the_end_of_input_is_a_token_the_conditional_grammar_rejects`
(parser.rs, nine rows) and the corpus case
`the-end-of-input-is-a-token-the-conditional-grammar-rejects.sh`, which walks the
three operand slots, a complete-but-unclosed conditional, nine group shapes for
the `(`-peek line rule, and the same set again without the continuation, where a
newline is the token instead.
