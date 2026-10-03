### TD-OILS-A-STRING-READERS-BACKSLASH-CLOSE-DOES-NOT-COST-A-FETCH. `-c` input ending in a lone `\` reads two lines low — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Spans::reader_stop` and the new
`parser::closed_with_backslash`; `userspace/oils/src/lexer.rs` —
`Lexer::reader_at_eof` and `Lexer::fetched_line`; `userspace/oils/src/interp.rs`
— `Shell::format_parse_error`. The close itself is `parser::close_last_line`,
and it is correct.

**What.** A *string* whose last line ends on a lone backslash is closed with a
second backslash rather than a newline (parse.y ~2570), so the pair is one
quoted literal backslash and there is no newline for the scan to stop on. bash
still counts the fetch that discovers that; osh did not. (Measured by writing
the probe to a file and sourcing it — a `-c` argument cannot carry a trailing
backslash through Windows' argv, so the same rows spelled `-c` are artifacts.)

```text
input, sourced                       bash                    osh (before)
echo 1⏎echo $LINENO\                 3\                      2\
echo 1⏎cat <<x\                      line 4: warning …       line 2: warning …
echo 1⏎cd $LINENO\                   line 3: cd: 3\: …       line 2: cd: 2\: …
echo 1⏎case x in  \                  line 4: unexpected      line 3: near unexpected
                                     end of file             token `newline'
```

The same inputs read from a *file* agree, because the stream reader appends a
newline and the trailing `\` becomes an ordinary continuation — which
TD-OILS-A-CONTINUATION-AT-END-OF-INPUT-DOES-NOT-MOVE-THE-READER now handles.

**Why.** The general rule the fix above implements is: a token's own lookahead
that exhausts the root input costs a fetch. After the `\\` close there is no
newline, so the word's terminator scan runs off the end of the buffer and enters
the fetch block — and then the parser's request for the next token enters it
again. That is two bumps, and osh currently records neither, because
`Spans::reader_stop` measures continuations and there is no continuation here:
the backslash pair is *text*.

**Fixed.** One predicate names the shape and three sites consult it.
`parser::closed_with_backslash` is "ends in a non-empty *even* run of
backslashes and not in a newline" — the exact signature of what
`close_last_line` writes for `InputKind::Str`, and of nothing else, since an
even run the user wrote is newline-closed like anything else. Then:

* `Spans::reader_stop` charges one line to the *last* token of such a text: its
  own look past the token runs the buffer out. This is the fetch a
  newline-closed input never makes, because it finds the newline the reader
  wrote back.
* `Lexer::reader_at_eof` answers true for it, so `run_into` stops appending the
  synthetic trailing `Tok::Newline` — bash has no newline token to hand over
  either, which is why `case x in  \` is an unexpected *end of file*. The second
  fetch is then the parser's own request, already charged by
  `Parser::reader_line_at`.
* `Lexer::fetched_line` adds two rather than one, because the continuation case
  it shares that branch with left its newline in the text and `cur_line` counts
  that character, while this one has no newline anywhere.

One consequence had to be modelled too: bash echoes the offending source line
from `shell_input_line`, and the fetch that ran the input out left it *empty* —
so `syntax error near unexpected token` blamed on a line past the last one is
followed by a literal `` `' ``. `print_offending_line` is called unconditionally
on that branch (parse.y:6263) and simply copies the empty buffer.
`format_parse_error` now echoes the empty line rather than omitting the line
entirely.

**Impact.** Line numbers only, and only for `-c`/`eval`/`.`/trap input whose
final line ends on an odd run of backslashes.

**Pinned by** two rows in `simple_command_line_follows_bashs_lookahead_rule`
(parser.rs) and the corpus case
`a-string-closed-with-a-backslash-has-no-newline-to-stop-on.sh`, which walks the
line stamped on a command, the same line in a diagnostic, an unterminated
here-document, six compounds that never close, three reserved words the close
turns back into ordinary words, and the end-of-file token itself.
