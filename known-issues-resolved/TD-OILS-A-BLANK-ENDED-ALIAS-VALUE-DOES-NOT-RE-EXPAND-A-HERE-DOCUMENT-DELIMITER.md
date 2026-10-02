### TD-OILS-A-BLANK-ENDED-ALIAS-VALUE-DOES-NOT-RE-EXPAND-A-HERE-DOCUMENT-DELIMITER. `alias S='cat << '` plus `S W` takes `W`, not `W`'s value — 2026-08-06 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/lexer.rs` — `expand_aliases_tracked`'s scan, which
tracks bash's `PST_ALEXPNEXT` (`AliasScan::force`) but does not apply it to the
word a `<<` is about to swallow as its delimiter.

**What.** An alias whose value ends in a blank makes bash try to expand the *next*
word too. When that next word is a here-document delimiter, it is expanded before
being taken as the delimiter. Measured:

```text
shopt -s expand_aliases
alias W='E9'
alias S='cat << '
S W
body-here
E9

bash: body-here                       — the delimiter is `E9`
osh : warning: here-document at line 4 delimited by end-of-file (wanted `W')
      body-here / E9 / echo "rc=0"    — the rest of the script eaten as body
```

**Why.** `read_token_word` is what reads the delimiter, and it runs the alias
check on the word it just read when `PST_ALEXPNEXT` is set — the delimiter is not
special to it. osh's scan clears `force` only on the ordinary command-word path,
and the delimiter word is consumed by `read_heredoc_delim` inside the lex, below
the level the scan works at, so no splice is ever attempted there.

**As fixed.** Two changes, and the first is the one that mattered.

*The delimiter is now a word with a span of its own.* `Spanned::starts` records,
for every token, the offset the *lexer iteration* that produced it began at — so
the `<<` and the `HereDoc` placeholder after it shared one, the operator's. That
made the delimiter unreachable in two separate ways: there was no span to write a
replacement over, and `AliasScan::pop_to`, which takes `PST_ALEXPNEXT` from every
push whose end lies in `(prev, start]`, never saw the pop, because the pop's end
sits between the operator and the delimiter and the delimiter claimed to start at
the operator. `Lexer::lex_heredoc_op` now records the delimiter word's own offset
(`Lexer::hd_delim`) and `Lexer::stamp_lines` stamps it over the iteration's. The
documented contract holds — `starts` promises "at or before the token's first
character", and this is exactly it. The one reader that wanted the *operator*'s
offset, `UngatheredHeredoc::op_offset` (it re-parses `src[..op_offset]` to ask
whether the `<<` stood at the top level), now takes it from a new
`PendingHeredoc::op_at` instead of reading it back off the placeholder token.

*The scan asks the question bash asks.* The candidate test was `Tok::Word` with a
single literal segment; it is now `alias_candidate`, which also answers for
`Tok::HereDoc(_, delim, false)` — an unquoted delimiter. That mirrors
parse.y:5266, `if (expand_aliases && quoted == 0) result = alias_expand_token
(token)`: bash asks it of whatever `read_token_word` just built, and `<<`'s target
is an ordinary WORD, so the delimiter is not special. Position remains the
caller's question and needs no change: `at_command()` is already false after a
redirection operator (`Prev::RedirOp`), so only `st.force` can make a delimiter a
candidate — which is bash's rule exactly, since reading the `<` clears
`PST_ALEXPNEXT` (parse.y:3511) and only a pop sets it again. Nothing else was
needed: the ordinary re-lex re-reads `cat << ` and finds the expanded delimiter,
and the `taken`/`rebuild` machinery unreads the body under it as it already did.

**Pinned by** `a_blank_ended_alias_value_expands_the_here_document_delimiter_after_it`
and the corpus case
`a-blank-ended-alias-value-expands-the-delimiter-after-it.sh`, which between them
probe `<<-`, a quoted delimiter (no lookup), a value *without* the trailing blank
(no flag), the flag being spent on the delimiter rather than reaching the operand
after it, a multi-word value, and a delimiter alias that names itself.
