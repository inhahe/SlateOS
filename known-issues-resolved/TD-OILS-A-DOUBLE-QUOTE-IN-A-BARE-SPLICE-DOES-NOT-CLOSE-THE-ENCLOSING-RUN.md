### TD-OILS-A-DOUBLE-QUOTE-IN-A-BARE-SPLICE-DOES-NOT-CLOSE-THE-ENCLOSING-RUN. `"${x:-$'a}b"c'}"` is `abc}` in bash — 2026-08-08 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/lexer.rs` (`ParseOpts::tolerant`),
`userspace/oils/src/parser.rs` (`word_tolerant_from_source_at`,
`segs_splice_past_the_brace`, `word_expanded_from_its_text`, `seg_to_parts`),
`userspace/oils/src/ast.rs` (`WordPart::TokenText`),
`userspace/oils/src/wordscan.rs` (`WordFault`, `word_fault`,
`backquote_extent`), `userspace/oils/src/interp.rs` (`Shell::expander_word`,
`Shell::dq_unclosed_backquote`).

**What.** Split out of
`TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT`,
which fixed the `}` case. The residue is the `"` case. When bash splices a
translated `$'…'` into a `${ … }` body **bare** (parse.y:3887), every character
of the translation is live at expansion time — including a `"`, which *closes*
the enclosing double-quoted run rather than sitting inside it as text. `osh`
carves the leftover out of the segment and re-reads it under the enclosing
quotes' rules, so its `"` stays literal. Measured (`x` unset throughout):

```text
                                            bash            osh
"${x:-$'a}b"c'}"                            abc}            ab"c}
"${x:-$'a}b"c d'}"                          abc d}          ab"c d}
y=Y; "${x:-$'a}b"$y'}"                      abY}            ab"Y}
"${x:-$'a}b"'}"                             ab}             ab"}
y='p q'; printf '[%s]' "${x:-$'a}b"$y'}"    [abp][q}]       [ab"p q}]
printf '[%s]' "${x:-$'a}b"*'}"              [ab*}]          [ab"*}]
printf '[%s]' "${x:-$'a}b"c"d'}"            [abcd}]         [ab"c"d}]
printf '[%s]' "${x:-$'a}b"c$'\x27'd'}"      [abcd}"]        [ab"c'd}]
printf '[%s]' "${x:-$'a}b"c"d"e'}"          [abcde}]        [ab"c"d"e}]
printf '[%s]' "${x:-$'a}b"c`echo Z'}"
      bash: bad substitution: no closing "`" in `echo Z}"
      osh : unexpected EOF while looking for matching `` ` ``
printf '[%s]' "${x:-$'a}b"c${'}"
      bash: "${x:-$'a}b"c${}": bad substitution
      osh : ${x:-a}b"c${}: bad substitution
```

Rows 5 and 6 are the ones with teeth: the leftover leaves the quoted run, so it
**splits** and **globs**. Rows 8 and 10 show why this cannot be repaired inside
the segment — the leftover swallows the enclosing `Seg::Dq`'s *own* closing `"`
(row 8's trailing `}"`), and row 10's backtick runs past it into the rest of the
word. No seg-local split models that.

**Fixed by** giving the *expander* its own read of the word's text, with
`expand_word_internal`'s tolerance, and letting the parser stop trying to
describe text it never read:

- `lexer::ParseOpts::tolerant` — a reader mode in which an unterminated `'` or
  `"` runs to the end of the string and says nothing, which is what
  `string_extract_single_quoted` (subst.c:1131) and
  `string_extract_double_quoted` (subst.c:963) do. Made a mode rather than a
  forked reader because the sibling entry needs the same one.
- `ast::WordPart::TokenText(Str)` (the renamed `CutAtNul`) — a whole word held as
  the **text of its token buffer**, because that text is not what the parser read
  and so cannot be described as a tree. Both producers now land here:
  `parser::segs_hold_a_nul` (the NUL cut) and `parser::segs_splice_past_the_brace`
  (a spliced `}` or quote), joined in `parser::word_expanded_from_its_text`.
  `unparse::word_src` already reproduces bash's token buffer byte for byte —
  verified by `declare -f` — so no new text-building machinery was needed.
- `parser::seg_to_parts`'s `BraceEnd::Early` segment split was **deleted**;
  `Early` and `Unclosed` now share one text path. That split was the wrong shape
  in principle, not merely incomplete: rows 7–10 show the leftover swallowing the
  enclosing `Seg::Dq`'s own closing `"` and running into the word's tail.
- `interp::Shell::expander_word`, called from `begin_word`, re-reads a
  `TokenText` word with `parser::word_tolerant_from_source_at` and expands *that*
  word. A text that will not read back even tolerantly keeps its text — that is a
  construct nothing closes, which the word-level scan has already reported.
- `wordscan::word_fault` replaces `unclosed_brace` and returns a `WordFault`, so
  the one left-to-right walk reports whichever fault comes first. `WordFault::
  Backquote` is row 10 (subst.c:11290), named from the backquote on because the
  call is `report_error (…, string + t_index)`; `wordscan::backquote_extent` is
  `string_extract (…, "`", SX_REQMATCH)` including its bare-trailing-backquote
  compatibility case. `interp::Shell::dq_unclosed_backquote` raises it with the
  same `arm_discard(1)` + errexit-only shape as the brace fault — measured: rc=1,
  the command DISCARDed, the shell continuing, `set -o posix` identical.

All eleven rows of the table above now match byte for byte.

**Relationship to the NUL cut.** A spliced NUL is the *other* character the bare
splice makes live. It was fixed first
(`TD-OILS-A-NUL-IN-AN-ANSI-C-STRING-DOES-NOT-TRUNCATE-THE-TRANSLATION`) for the
shapes where the cut leaves a construct **open**, where the word is doomed before
any of the splitting/globbing questions rows 5 and 6 turn on. The well-formed
shapes needed this same reader, and were fixed with it —
`TD-OILS-A-CUT-WORD-IS-EXPANDED-AS-LITERAL-TEXT` below.
