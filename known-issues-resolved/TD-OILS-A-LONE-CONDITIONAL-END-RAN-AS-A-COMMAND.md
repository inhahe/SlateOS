### TD-OILS-A-LONE-CONDITIONAL-END-RAN-AS-A-COMMAND. `]]` was an ordinary word to osh where bash makes it a reserved token — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/parser.rs` — the `RESERVED` table, which listed
every reserved word bash's `word_token_alist` holds *except* `]]`.

**What.** `]]` written where a command could start is a syntax error in bash
and was a command called `]]` in osh:

```
$ echo 'true; ]]' > s.sh
bash: s.sh: line 1: syntax error near unexpected token `]]'
      s.sh: line 1: `true; ]]'                              rc=2
osh : s.sh: line 1: ]]: command not found                   rc=127
```

Every command position was affected — a bare `]]`, after `;`, after `&&`,
`||` or `|`, inside `{ }` or `( )`, and the condition or body of `if`,
`while`, `until`, `for` and `case`. Surveying all 21 of bash's reserved
words in command position found `]]` to be the only one osh got wrong.

**Why.** `CHECK_FOR_RESERVED_WORD` (parse.y:2994) looks a word up in
`word_token_alist` — which lists `{ "]]", COND_END }` right beside `fi`,
`done` and `}` — on the single condition that

```c
    if (!dollar_present && !quoted && \
	reserved_word_acceptable (last_read_token)) \
```

i.e. the parser is at the start of a command. Nothing in the macro asks
whether a `[[` is open; the one line that names the conditional state only
clears it (`parser_state &= ~(PST_CONDCMD|PST_CONDEXPR)`). So `]]` is the
token `COND_END` in command position always, the grammar has no production
beginning with it, and the parse dies — the same reason a lone `fi` is an
error rather than a command named `fi`.

The other half of the rule is why the entry belongs in `RESERVED` and
nowhere else: `reserved_word_acceptable` is false past the command word, so
`echo ]]`, `x=]]`, `for x in ]]` and `case ]] in ]])` are all ordinary words
— which osh already had right, and which `RESERVED` membership preserves
because `parse_simple` only consults it before the first word. Quoting also
takes the token away (`!dollar_present && !quoted`), so `"]]"`, `\]]` and
`$q` with `q=]]` are commands whose name is `]]` — `command not found`, not
a syntax error.

**Fix.** One entry added to `RESERVED`. `parse_command` already routes a
reserved word that introduces no compound to `unexpected_here()`, which is
bash's diagnostic verbatim, so nothing else had to change.

**Instrumented by**
`userspace/oils/tests/corpus/a-lone-conditional-end-is-a-reserved-word-wherever-a-command-could-start.sh`,
which measures all four halves of the rule: the thirteen command positions,
the six word positions, the four quoted spellings, and that `[[ … ]]` still
closes.
