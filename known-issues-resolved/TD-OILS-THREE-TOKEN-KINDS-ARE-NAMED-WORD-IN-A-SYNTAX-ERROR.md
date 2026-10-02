### TD-OILS-THREE-TOKEN-KINDS-ARE-NAMED-WORD-IN-A-SYNTAX-ERROR. `f() 007>x` and `f() a=(1   2)` name the wrong token, and `f() {v}>x` the wrong message — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `token_display_at`'s catch-all arm,
and `unexpected_here`, which had only one of bash's two message shapes.

**What.** `error_token_from_token` (parse.y:6132–6169) is a switch with a branch
per token kind, and osh had ported four of them; the rest fell into a catch-all
that prints the literal string `word`. Three kinds reach it, and each is wrong in
a different way.

**A `NUMBER` is named by its value.** The branch is `t = itos (yylval.number)`,
so the digits are re-rendered from the parsed integer and the spelling is gone:

```text
f() 007>x            bash: near unexpected token `7'      osh: near `word'
f() 2>/dev/null      bash: near unexpected token `2'      osh: near `word'
f() 12>&1            bash: near unexpected token `12'     osh: near `word'
```

The leading zeros are the discriminator — nothing that echoed the source could
produce `7` from `007`.

**A compound `ASSIGNMENT_WORD` is named by a re-print too.** The branch is
`yylval.word->word`, but for `a=( … )` that word was never the source:
`read_token_word` builds it as the name, then `=`, `(`, the `string_list` of the
elements, and `)` (parse.y:5168–5181), and `string_list` joins with exactly one
space (parse.y:6572–6576). So the gaps are normalised while each element keeps
its own spelling:

```text
f() a=(1   2)        bash: near unexpected token `a=(1 2)'    osh: near `word'
f() a=(  )           bash: near unexpected token `a=()'       osh: near `word'
f() a=('x y' "z")    bash: near unexpected token `a=('x y' "z")'
f() a=([2]=v x)      bash: near unexpected token `a=([2]=v x)'
f() a+=(p)           bash: near unexpected token `a+=(p)'
f() a[1+1]=(q)       bash: near unexpected token `a[1+1]=(q)'
```

**A `REDIR_WORD` is not in the switch at all**, so the function returns NULL and
`report_syntax_error` falls through to its text-scanning branch — which prints a
*different message*, without `unexpected token`, and names the scan's text rather
than the token (parse.y:6251 against 6276). osh had no such fall-through:

```text
f() {v}>/dev/null    bash: syntax error near `{v}>'
                     osh:  syntax error near unexpected token `word'
f() {v}<in           bash: syntax error near `{v}<'
```

The trailing `>` comes along because the near scan reaches one character past the
token, which is the same rule the conditional's `near` already implements.

**Impact.** Every one of these is a diagnostic a script author reads to find
their mistake, and `word` names nothing at all.

**Fixed.** `token_display_at` gained the `NUMBER` and compound-`ASSIGNMENT_WORD`
branches, the latter re-printing the literal the way `read_token_word` builds it
rather than slicing the source — which is also why it needs no source extent, a
thing the token stream does not carry. And `unexpected_here` now mirrors
`report_syntax_error`'s structure instead of collapsing it: a new `error_token_at`
is the port of `error_token_from_token` including its NULL, and where it declines
to name a token the near-scan branch runs and prints the shorter message. Only
`REDIR_WORD` reaches that today, which is exactly bash's own set — every other
kind the parser can be holding has a branch in the switch.

**Pinned by** the parser test
`a_number_and_a_compound_assignment_are_named_by_their_reprint` and the corpus
case `a-number-and-a-compound-assignment-are-named-by-their-reprint.sh`.
