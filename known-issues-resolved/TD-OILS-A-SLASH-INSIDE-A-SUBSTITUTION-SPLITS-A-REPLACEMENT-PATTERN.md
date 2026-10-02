### TD-OILS-A-SLASH-INSIDE-A-SUBSTITUTION-SPLITS-A-REPLACEMENT-PATTERN. `${s/$(echo a/b)aaa/Y}` fails to lex — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/parser.rs` — `parse_replace_pieces`.

**What.** The pattern of a `${var/pat/repl}` runs to the next unescaped `/`, and
`parse_replace_pieces` finds it with a flat scan that honours only `\/`. It does
not step over a nested `$( … )`, `${ … }`, `` `…` ``, `'…'` or `"…"`, so a `/`
inside one of those ends the pattern early. The two halves are then each lexed,
and the truncated pattern is an unterminated substitution:

```text
s=zaaaz; echo "[${s/$(echo a/b)aaa/Y}]"
bash: [zaaaz]
osh : line 4: unexpected EOF while looking for matching `)'
```

A `/` in the *replacement* half is fine (`${s/aaa/$(echo a/b)}` matches), because
nothing splits that half further. Backticks fail the same way
(``${s/`echo a/b`aaa/Y}``), and so does a nested brace (`${s/${x:-p/q}aaa/Y}`).

bash has no such scan: `parse_matched_pair` already recorded where each nested
construct ends while reading the body, and the pattern/replacement split in
`expand_word_internal` walks that structure rather than raw characters.

**Fixed by** a "skip one construct starting at `i`" helper over `&[Ch]` —
`skip_construct`, with `skip_quoted` and `skip_balanced` under it — which
`parse_replace_pieces` now consults at every position, copying any construct it
finds into the current half whole. The scan stays a *character* scan (the halves
are lexed separately afterwards) and deliberately shallow: it finds each
construct's extent and nothing else, which is all a split needs.

The backslash keeps exactly one special case: `\/` in the pattern half, which is
the one place a backslash escapes *this scan's own* delimiter and so is consumed.
Every other escape is the later lex's to interpret and is copied whole. A
construct that does not close (`None`) is copied a character at a time and left
to that later lex — unreachable in practice, since an unterminated quote or
substitution inside a `${ … }` is already a lexer error before the body is
handed over.

`frag_line` needed no change: it counts newlines over the parent slice by index,
so skipping a multi-line construct still counts its newlines.

Corpus case:
`userspace/oils/tests/corpus/a-replacement-separator-is-found-at-one-level-only.sh`.

**Sibling, fixed with it:** `matching_subscript_close` had the same shape — it
skipped quoted runs but not substitutions, so `${a[$(echo 1])]}` split at the
inner `]`. It is now one line over `skip_balanced`. See
`a-subscript-close-is-found-at-one-level-only.sh`; the divergence there is
visible in the *kind* of failure, since bash fails the whole `$(echo 1])`
subscript as arithmetic (rc=1) where osh failed to lex the truncated one (rc=2).
