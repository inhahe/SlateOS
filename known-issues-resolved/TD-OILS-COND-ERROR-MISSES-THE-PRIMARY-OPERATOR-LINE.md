### TD-OILS-COND-ERROR-MISSES-THE-PRIMARY-OPERATOR-LINE — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/parser.rs`, `Parser::cond_operand_error`,
the `CondPos::Primary` arm.

**What.** When the *first* thing inside `[[ ]]` is a control operator, bash
prints a first diagnostic line that osh omits entirely:

```text
[[ ;Q ]]   bash: unexpected token `;' in conditional command   osh: (absent)
           bash: syntax error near `;Q'                        osh: syntax error near `;Q'
           bash: `[[ ;Q ]]'                                    osh: `[[ ;Q ]]'
[[ ; ]]    same shape                                          same omission
[[ |Q ]]   unexpected token `|' in conditional command         (absent)
```

Note the wording: `in conditional command`, not the
`, conditional binary operator expected` that a *second*-position operator gets
(`[[ P;Q ]]`) — which osh does print. The `near` line and the echoed line
already match, so this is one missing line.

**Impact.** Cosmetic; one diagnostic line.

**Fixed 2026-08-05.** `cond_primary_token` says how bash names the token, and
`CondPos::Primary` prepends the clause when it does. The rule turned out to be
simpler than "control operator": bash's `cond_term` tries everything that can
*begin* a term (`]]`, `(`, `!`, a unary operator, a word) and names whatever is
left, which is every operator — including the redirection ones, so `[[ >Q ]]`
and `[[ &>Q ]]` are named too. The one word that is not a term, the `]]` closer,
leaves through an earlier arm and prints no such line, and end of input prints
one spelled `EOF`.

Two tokens bash cannot spell: `error_token_from_token` has no text for
`IO_NUMBER` or `REDIR_WORD` (they carry a number and a word rather than a fixed
spelling), so bash falls through to `%d` of the raw yacc token number and
`[[ 2>Q ]]` says `unexpected token 284`, unquoted. osh reproduces both numbers
from bash 5.2's generated `y.tab.h`.

Primary position is wherever a *term* is expected, not just the first one, so
`[[ ! ;Q ]]` and `[[ P == P && ;Q ]]` get the same line.

**Pinned by** `tests/corpus/a-conditional-names-the-token-that-cannot-begin-a-term.sh`.
