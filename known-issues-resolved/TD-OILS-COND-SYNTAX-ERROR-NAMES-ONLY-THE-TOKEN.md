### TD-OILS-COND-SYNTAX-ERROR-NAMES-ONLY-THE-TOKEN. `[[ P;Q ]]` says ``near `;'`` where bash says ``near `;Q'`` — 2026-08-04 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/parser.rs` — the `[[ ]]` conditional parser's error
reporting, which names the offending token as the lexer read it.

**What.** bash's second diagnostic line names the token *together with the first
character of whatever follows it*, where osh names the bare token. The first
line (`unexpected token …, conditional binary operator expected`) and the third
(the echoed source line) already match.

```text
[[ P;Q ]]     bash: syntax error near `;Q'     osh: syntax error near `;'
[[ P;QRS ]]   bash: syntax error near `;Q'     osh: syntax error near `;'
[[ P;$n ]]    bash: syntax error near `;$'     osh: syntax error near `;'
[[ P;"Q" ]]   bash: syntax error near `;"'     osh: syntax error near `;'
[[ P;;Q ]]    bash: syntax error near `;Q'     osh: syntax error near `;'
[[ P|Q ]]     bash: syntax error near `|Q'     osh: syntax error near `|'
[[ P&Q ]]     bash: syntax error near `&Q'     osh: syntax error near `&'
[[ P; Q ]]    bash: syntax error near `;'      osh: syntax error near `;'   — agree
[[ P;\nQ ]]   bash: syntax error near `;'      osh: syntax error near `;'   — agree
```

A blank or a newline after the token is where the two agree, which is why every
existing corpus case that provokes this error matches: they are all written with
a space.

**Why bash does it.** `report_syntax_error` in bash's `parse.y` does not print
the parsed token — it calls `error_token_from_text`, which reconstructs a token
by walking the raw `shell_input_line` backwards from the current read index. The
index has already moved past the lexer's one-character lookahead, so the text it
recovers carries that extra character. The `;;Q` row (reported as `;Q`, not
`;;Q`) is the same reconstruction seen from the other side: in `[[ ]]` the lexer
reads `;` singly, so the walk starts inside the pair.

**Impact.** Cosmetic — the message text of a syntax error inside `[[ ]]`. Exit
status, the other two lines, and every non-error behaviour are unaffected.

**Fixed 2026-08-05** by giving the conditional parser the same reconstruction —
the prescribed fix, not a patch on the token text. `error_token_from_text` is
ported whole as `Spans::near` in `src/parser.rs`: start one past the offending
token, step back off the terminating NUL, skip back over ` `/`\t`/`\n`, set
`token_end = i + 1`, scan back while the character is *not* in `" \n\t;|&"`,
then skip forward over whitespace while `i != token_end`. To feed it, the lexer
now returns a named `Spanned { toks, lines, ends }` (was a widening tuple) and
the parser carries a `Spans { src, ends }` beside its token stream.

Three things the port settles that a token could not have:

- **Exactly one character after the token comes along, and no more.** `[[ P;QRS ]]`
  is `;Q` because the forward skip stops at `token_end`.
- **Text written flush *before* it comes along too.** `[[ a>>b ]]` is `a>>b` and
  `[[ -n @(a) ]]` is `@(a` — parentheses, quotes and redirection characters are
  not delimiters, so the backward scan sweeps them up.
- **A delimiter inside a multi-character operator cuts it short**: `[[ P;;Q ]]`
  is `;Q` and `[[ a>|b ]]` is `|b`, while `>>` and `<<<` come through whole.

Scoping note worth keeping: the backward scan always stops at or after the space
that must follow the `[[` reserved word, so it can never escape the conditional's
body. That is why a `$( … )` or `<( … )` body needs only its *own* text, even
though bash's `shell_input_line` there is the enclosing line — measured, before
any plumbing was written (`echo $([[ a>>b ]])` reports `a>>b`, not `$([[ a>>b`).

An alias-expanded parse splices in tokens that were never written where they are
being read — but they *were* written somewhere, in the alias's value, and bash
reads that value by pushing it onto the input, so the scan runs over it. The
spans therefore name a *text* as well as an offset; see
TD-OILS-COND-ERROR-NEAR-IGNORES-THE-ALIAS-TEXT for that rule.

**Pinned by** `tests/corpus/a-conditional-error-quotes-the-source-not-the-token.sh`.
