### TD-OILS-COND-ERROR-DROPS-THE-EXPECTED-PAREN-LINE — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/parser.rs`, `Parser::parse_cond_primary` — the
`Op::LParen` branch, which propagates the inner parse's error with `?`.

**What.** When a conditional fails *inside* a `( … )` group, bash prints one
more line than osh does. Its `cond_term` reports the inner failure and returns
an error; the group's own arm then still checks for the `)` it never reached and
reports that too; only afterwards does the top level add the `near` line:

```text
[[ ( ;Q ) ]]   bash: unexpected token `;' in conditional command
                     expected `)'
                     syntax error near `;Q'
                     `[[ ( ;Q ) ]]'
               osh:  (same, without the `expected `)'` line)

[[ ( a         bash: unexpected token `newline', conditional binary operator expected
                     expected `)'
                     syntax error near `a'
                     `[[ ( a '
               osh:  (same, without the `expected `)'` line)
```

Note where the missing line goes: *between* the inner diagnostic and the `near`
line, not after it. osh already prints `unexpected token `X', expected `)'` when
the group parses fine but no `)` follows (`[[ ( a ) b ]]`), so it is only the
cascade — an inner error passing through a group — that loses it.

**Why.** osh builds the whole diagnostic as one string in the erroring frame:
`cond_operand_error` glues the `near` line onto its own message and returns it,
and the group's `?` re-raises that string unchanged. There is no place left to
insert a clause between the two lines.

**Impact.** Cosmetic; one diagnostic line, and only for a conditional that fails
inside parentheses.

**Fixed 2026-08-05.** A conditional diagnostic is no longer a string until it is
printed: `CondError` carries the clauses and the last line separately, the
`[[ … ]]` grammar returns it in place of a `ParseError`, and a group's frame
appends `expected `)'` as the error passes out through it. `parse_cond` renders
whatever arrives. TD-OILS-COND-ERROR-NEAR-IGNORES-THE-ALIAS-TEXT now has
somewhere to put the alias's own text as well.

Two things fell out that the measurement above had not reached. One clause
arrives per group the error passed through — three for `[[ ( ( ( ;Q ) ) ) ]]` —
and each is reported at the line its *own* `(` was on rather than the line the
failure was found on, because bash's `cond_term` saves `line_number` on entry
and hands the saved one to `parser_error`. A multi-line diagnostic therefore
does not have a single line number, so `ParseError::line_at` records the message
lines that differ from the error's own and `format_parse_error` tags each with
its own.

That same frame also names the token when its contents *did* parse and the input
then ran out, which osh had been dropping altogether: `[[ ( -n x ` says
`unexpected token `EOF', expected `)'` before the end-of-file line.

**Pinned by** `tests/corpus/a-conditional-group-adds-the-paren-it-never-reached.sh`.
