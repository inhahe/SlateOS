### TD-OILS-A-CUT-WORD-IS-EXPANDED-AS-LITERAL-TEXT. `"${x:-$'a}b\0c'}"` is `ab` in bash, `"${x:-a}b` in osh — 2026-08-08 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs` — `Shell::expander_word`, called from
`Shell::begin_word`; `userspace/oils/src/lexer.rs` (`ParseOpts::tolerant`) and
`userspace/oils/src/parser.rs` (`word_tolerant_from_source_at`).

**What.** The residue of
`TD-OILS-A-NUL-IN-AN-ANSI-C-STRING-DOES-NOT-TRUNCATE-THE-TRANSLATION`. A NUL cut
leaves the *word's text* short, and bash then expands that text with
`expand_word_internal` like any other. osh models the cut as
`ast::WordPart::TokenText(raw)` (then named `CutAtNul`) and expands `raw` as
**literal text**.

That is right whenever the cut leaves a construct open, because the word-level
scan `Shell::begin_word` runs over the same text has already raised
``no closing `}'`` and armed the DISCARD — nothing downstream can observe what
the part contributed. But a spliced `}` can close the expansion *before* the NUL
arrives, and then the cut text is a perfectly good word:

```text
                                              bash        osh
printf '[%s]' "${x:-$'a}b\0c'}"               [ab]        ["${x:-a}b]
printf '[%s]' A"${x:-$'a}b\0c'}"B             [Aab]       [A"${x:-a}b]
printf '[%s]' "${x:-$'}\0c'}"                 []          ["${x:-}]
printf '[%s]' "${x:-$'a}b\0c'}" second        [ab][second][ "${x:-a}b][second]
```

(The `B` and the `c'}"` really are gone in bash too — that is the cut, and it is
modelled correctly. What is wrong is only that the remaining text is not read.)

**Fixed by** re-reading the word's text at expansion time, with the tolerant
reader the sibling entry needed. `Shell::expander_word` sends a
`WordPart::TokenText` word (the renamed `CutAtNul`) through
`parser::word_tolerant_from_source_at` and expands the word *that* read carves;
the cut text is by construction unterminated (the cut removed the closing `"`),
which is exactly what the tolerance is for. Doing this for the cut alone would
have forked the word reader for one part kind, so it was built as a `ParseOpts`
mode and both entries were closed together — see
`TD-OILS-A-DOUBLE-QUOTE-IN-A-BARE-SPLICE-DOES-NOT-CLOSE-THE-ENCLOSING-RUN` above
for the full account. All four rows of the table above now match byte for byte.
