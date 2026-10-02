### TD-OILS-A-NUL-IN-AN-ANSI-C-STRING-DOES-NOT-TRUNCATE-THE-TRANSLATION. `"${x:-$'a\0b'}"` is a bad substitution in bash — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/escape.rs` (`ansi_c_translate` / `ansi_c_unescape`
/ `cut_at_nul`), `userspace/oils/src/lexer.rs` (`read_ansi_c_source`, and
`read_dollar_brace_body`'s bare-splice arm), `userspace/oils/src/parser.rs`
(`segs_hold_a_nul`, `word_expanded_from_its_text`, `comsub_reprint_error`,
`word_list_lex_error`), `userspace/oils/src/ast.rs` (`WordPart::TokenText`,
then named `CutAtNul`),
`userspace/oils/src/interp.rs` (`comsub_reparse_error`,
`array_assign_reparse_error`).

**What.** Also split out of
`TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT`.
bash's `ansiexpand` yields a `(char *, size_t)` pair, and which of the two a
caller keeps is the whole story. Almost every caller keeps the pointer, so a
`\0` in the source ends the *translation* and the word runs on unharmed. Exactly
one keeps the length —

```c
      nestret = ansiexpand (…, &ttranslen);   /* parse.y:3890 */
      …
      nestlen = ttranslen;                    /* parse.y:3892 */
```

— the **bare** splice of a `$'…'` into a double-quoted `${ … }` body, row three
of the sibling entry's table. That is the one path that puts a NUL in the token
buffer, and `make_word` then copies the buffer with `savestring`, so the **word**
ends at it: the `}` and the closing `"` the script wrote are gone.

```text
                    bash                                osh (before)
"${x:-$'a\0b'}"     no closing `}' in "${x:-a           a
```

**The recorded hypothesis was wrong, and the measurement won.** The old text
said the fix was "narrow and local: truncate at the first NUL **at the splice**".
That is right for every *other* row — and is what `ansi_c_unescape` now does —
but it is not what happens here. The cut is **word-scoped, at the token
boundary**: the word's *text* is truncated while its *extent* is not, so the
words and commands after it are read exactly as they would have been. That
distinction is the whole of the observable behaviour, and a splice-local truncate
cannot produce it.

**Fixed by** carrying both answers and cutting where bash cuts:

- `escape::ansi_c_translate` is `ansiexpand`'s length answer;
  `escape::ansi_c_unescape` is `cut_at_nul` of it, the C-string answer. Every
  pre-existing caller keeps the latter, so nothing else moved.
- `lexer::read_ansi_c_source` is split out of `read_ansi_c_quote` so the
  bare-splice arm of `read_dollar_brace_body` can ask for the untruncated
  translation; every other arm still re-quotes through `sh_single_quote`.
- `parser::word_expanded_from_its_text` (then named `word_cut_at_nul`), run at
  the end of `word_from_segs_in` when `segs_hold_a_nul`, replaces the word's
  parts with a single `ast::WordPart::TokenText(raw)` holding the word's source
  text up to the NUL.
  It is unparsed source, so `declare -f` prints the cut word back exactly as bash
  does, and the expander re-reads it as text.
- The cut leaves constructs open, and three readers meet the wreckage. Each is
  now modelled where bash models it:
  - `parameter_brace_expand`'s scan, at expansion time — ``bad substitution: no
    closing `}'`` naming the word *as cut*. This already fell out of the existing
    word-level scan.
  - `xparse_dolparen`, when the cut fell inside a `$( … )` body, whose re-print
    is then unclosed — `parser::comsub_reprint_error`, see the sibling entry
    `TD-OILS-A-CMDSUB-REPRINT-IS-RE-READ-WITH-ITS-TAIL` below.
  - `parse_string_to_word_list`, when the cut word is an element of a compound
    array assignment — `parser::word_list_lex_error` +
    `interp::array_assign_reparse_error`.

Corpus cases:
`userspace/oils/tests/corpus/a-nul-in-a-bare-spliced-translation-cuts-the-word.sh`,
`userspace/oils/tests/corpus/a-compound-array-assignments-value-list-is-re-read-as-a-word-list.sh`,
and the new sections of
`userspace/oils/tests/corpus/a-cmdsub-body-is-parsed-twice-and-the-second-parse-reads-the-reprint.sh`.
