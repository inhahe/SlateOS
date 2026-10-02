### TD-OILS-ARRAY-ASSIGN-REREAD-ORDER. A compound assignment's two re-read failures are not ordered by position — 2026-08-08 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs` — `Shell::array_assign_reparse_error`.

**What.** `assign_compound_array_list` re-reads a compound assignment's joined
value list with `parse_string_to_word_list` (arrayfunc.c:587), and that read can
fail two ways: the listing's own tokenizer meets an unclosed construct
(a `DISCARD`, `$?` = 1), or a `$( … )` in it has a re-print that will not parse
back (`parse_comsub`'s `FORCE_EOF`, the shell ends). bash meets whichever comes
**first in the listing**, because it is one left-to-right read.

osh ran the two checks in sequence instead: `parser::word_list_lex_error` over
the whole listing first, then a scan of the listing's command substitutions. Both
orderings diverged:

```text
a=("$(⏎!⏎)" "${x:-$'a\0b'}")   bash: syntax error near unexpected token `)' , rc=2, shell ends
a=("${x:-$'a\0b'}" "$(⏎!⏎)")   bash: syntax error near unexpected token `)' , rc=2, shell ends
osh (both)                     unexpected EOF while looking for matching `}' , rc=1, continues
```

**The recorded hypothesis was wrong, and the measurement corrected it.** The
entry claimed the old order "is correct whenever the unclosed construct precedes
the bad re-print — an unclosed construct swallows everything after itself, so no
substitution beyond it is ever reached as one." It does not: the second row above
is bash reading the listing while standing *inside* the `${` the cut left open,
and it still recurses into the `$(` past it. The substitution wins **always**, and
not because of where it stands — `parse_comsub` fails where the `$( … )` is,
whereas the tokenizer's failure is an *end of input* failure by construction
(`parse_matched_pair` only gives up once the listing runs out), so the
substitution is unconditionally the earlier of the two.

Nothing can hide a substitution from the read, either. The constructs the
tokenizer scans opaquely — `'…'` and `` ` … ` `` — are exactly the ones it does
not translate a `$'…'` inside (the matched pair is taken whole, untranslated), so
a NUL can never be spliced into one and a cut can never leave one open. The only
constructs a cut can leave open are `${` and `"`, and the read recurses into a
`$(` from inside either.

**Fixed by** swapping the two checks — substitutions first, tokenizer second —
and deleting the ordering caveat from the comment. The proposed byte-offset fix
below was therefore **unnecessary**: ordering by offset buys nothing when one of
the two offsets is always EOF. (The offset may still be worth carrying on
`lexer::LexError` for other diagnostics; that is now its own question, not this
one's.) Cost is neutral or better: `word_list_lex_error` tokenizes the whole
listing, and the usually-empty `array_listing_comsubs` walk now runs in front of
it and short-circuits, so the listing is not even rendered unless it is needed.
Corpus coverage: four new sections in
`tests/corpus/a-compound-array-assignments-value-list-is-re-read-as-a-word-list.sh`
(both orderings, the class change `eval` sees, and the backquote body that hides
nothing).

**Superseded fix (not taken).** Carry a byte offset on `lexer::LexError` — it
already carries a line — so the tokenizer's failure can be ordered against the
offset of the first bad substitution in the rendered listing, and the earlier of
the two reported.
