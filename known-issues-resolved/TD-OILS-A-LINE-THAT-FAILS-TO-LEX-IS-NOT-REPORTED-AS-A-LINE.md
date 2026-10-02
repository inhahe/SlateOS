### TD-OILS-A-LINE-THAT-FAILS-TO-LEX-IS-NOT-REPORTED-AS-A-LINE. An unclosed quote hides the syntax error before it — 2026-08-06 — ✅ FIXED 2026-08-07 (`set -v` half and syntax-error half)

**Where:** `userspace/oils/src/lexer.rs` — `tokenize_deferred`, which lexes the
whole input up front and, on an unterminated construct, cuts the tokens back to
the last *complete* logical line; and `userspace/oils/src/parser.rs` —
`IncrementalParser`, which therefore never sees a token from the failing line.

**What.** bash's reader hands a physical line over as soon as it has read it, and
the parser then pulls tokens off it one at a time; the unterminated construct is
only discovered when the token containing it is *asked for*. osh lexes first and
parses second, so anything the failing line would have caused before the
construct was reached is lost. Two measured shapes, neither involving
here-documents:

```
                    bash                                    osh
) echo x            near `)' + echo `) echo x'              same                  ✔
) echo "            near `)' + echo `) echo "'              unexpected EOF … `"'  ✘
set -v; echo hi     echoes `echo hi', runs it               same                  ✔
set -v; echo "      echoes `echo "', then unexpected EOF    the error only        ✘
```

In the second row bash's yacc reads `)`, errors on it, and never asks for the
token that would have opened the quote — so the quote is never lexed and never
reported. In the fourth, `set -v` echoes in the *reader* (bash's `shell_getc`),
before any token is taken from the line, so the echo happens whatever the line
turns out to contain.

Found while measuring shapes for
`tests/corpus/a-here-document-the-reader-never-reached-is-warned-about-below-the-error.sh`
(where the `) cat <<E "` probe is deliberately left out for it). Note the
here-document warning itself is *not* affected: it is correctly suppressed there,
because the prefix in front of the `<<` is not a complete program either.

**The `set -v` half is ✅ FIXED (2026-08-07)** — in `parser.rs`, not `lexer.rs`.
Measuring it showed the entry had understated the bug: the echo was wrong for
*grammar* errors too, not only for a lexer death, in three shapes with one cause.

```
                           bash echoed        osh echoed
fi                         the line           nothing
) echo x                   the line           nothing
echo a; ) bad              the whole line     `echo a;'   (cut at the error)
{ echo a / ) bad / }       both lines         the first only
echo "unterm / echo three  both lines         nothing
```

`IncrementalParser::split_unit_lines` ended a unit's raw span at the last
**successfully consumed** token. But `shell_getc` hands the parser a whole
physical line and echoes it *there*, before a single token is taken off it — so
by the time yacc finds a token it cannot shift, that token's line has already
gone out. An error on the unit's first token therefore left the span empty, a
mid-line one cut the line in half, and an error on a later line of a multi-line
unit dropped that line entirely.

An error-ending unit now goes through `split_unit_error_lines`, which stretches
the span to the end of the physical line the **offending token** stands on.
Taking the offending token — rather than just running the old end out to its own
line's end — is what the `{ echo a` / `) bad` shape needs: the parse stopped on a
`Newline`, so the old end was *already* at a line boundary and the reader had
simply gone on to read the next line before failing on it. The parked-lexer-error
unit takes the same path with end-of-input as its blame, which is exactly right:
the reader ran to EOF looking for the close, so every line it passed belongs to
the unit that reports it. The same span feeds the command history, and bash
agrees there too (measured through `eval` under `set -o history`).

**Pinned by** `interp.rs::a_unit_that_fails_to_parse_still_covers_the_line_it_failed_on`
and `tests/corpus/set-v-echoes-a-line-the-parse-then-refuses.sh`.

**The syntax-error half is ✅ FIXED (2026-08-07)**, and turned out to want no
bail token at all — just the removal of the cut, plus a sharper rule for when the
parked error is due.

`tokenize_deferred` no longer truncates `toks` back to the last complete logical
line. Everything the scan produced is kept, the failing line's own tokens
included, because bash's parser has already *been handed* them one at a time by
the time the reader chokes on what follows. The one surviving cut is a
here-document still awaiting its body: its `<<` left a placeholder token that was
never filled in, and that must not reach the parser, so the line it stands on
goes with it.

`IncrementalParser::next_unit` then decides between the parked error and whatever
the unit came to on a single question — did the parse **run dry**, i.e. ask for a
token the stream did not have? That fetch is the one that would have found the
lexer's error, so a parked error replaces the outcome whenever it happens: a
grammar error the truncated stream provoked (`if true; then` + `echo 'unterm`), a
clean parse of the commands in front of the construct (`echo one; echo 'unterm`
runs neither), or the end of input itself. What survives is an error raised over
a token the parser did get — `) echo "` reports the stray `)`.

Three details are load-bearing, each measured:

* **"ran dry" is not "stopped at the last token."** A unit ended by a newline
  that happens to be final fetched nothing past it and is a complete unit that
  runs: `echo one` / `echo two` / `v='abc` still prints both. So the flag is set
  by the loop at the two breaks that mean end-of-stream, not by testing the
  cursor afterwards.
* **A grammar error counts as dry only if it has the *shape* of running out**
  (`ParseError::is_incomplete`). An eagerly-parsed `$( … )` body's own objection
  is a real error over a token that was fetched, and must survive:
  `echo $(fi) "` reports `fi`, as bash does.
* The lexer-error arm still spans the whole remaining input for `set -v` and the
  history (`split_unit_error_lines(orig.len(), orig.len())`) and still abandons
  the rest of the input, since it is fatal.

**Also pinned by** `parser.rs::a_line_that_fails_to_lex_still_offers_the_tokens_it_had`
and `tests/corpus/a-line-that-fails-to-lex-still-offers-the-tokens-it-had.sh`
(27 shapes, including the here-document warnings and the `&`-backgrounded
output-after-diagnostic ordering).
