### TD-OILS-A-CONTINUATION-AT-END-OF-INPUT-DOES-NOT-MOVE-THE-READER. `$LINENO` and EOF diagnostics read one or two lines low — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Parser::reader_line_at`,
`Parser::parse_tokens_ending` and the streaming parser's error stamp;
`userspace/oils/src/lexer.rs` — the new `Lexer::reader_at_eof`, the three
terminal branches of `Lexer::run_into`, and `Lexer::fetched_line`.

**What.** Two related divergences, both about what the reader has done by the
time the parser asks for its next token at end of input.

```text
input (a script file)              bash                       osh
echo 1⏎echo L$LINENO\              L3                         L2
echo 1⏎echo L$LINENO\⏎             L3                         L2
echo 1⏎echo L$LINENO\⏎⏎echo 3      L3                         L2
echo 1⏎nosuch$LINENO\              line 4: nosuch4: …         line 3: nosuch3: …
echo 1⏎cat <<x\                    line 4: warning … `x'      line 3: warning … `x'
echo 1⏎cat >\⏎                     line 3: syntax error:      line 4: syntax error near
                                   unexpected end of file     unexpected token `newline'
echo 1⏎cat <<\                     line 3: syntax error:      line 4: syntax error near
                                   unexpected end of file     unexpected token `newline'
```

Note the third row: this is **not** an end-of-file-only effect. A word whose
last character is followed by `\⏎` and then a *blank* line is numbered by bash
one higher, because the scanner asked for another word character, the reader ate
the continuation to answer, and only then returned the newline that ends the
word. osh already agrees whenever real characters follow the continuation
(`echo a\⏎b $LINENO` → 2 in both), so what is missing is only the case where
nothing does.

**Why.** bash's `line_number` is bumped in two places in `shell_getc` (parse.y):

* 2361, on entry to the block that fetches a new physical line — which is
  reached **whenever the buffer is exhausted**, including the empty fetches
  after end of file. Each further token request at EOF therefore bumps it
  again, which is where the second of the two lines comes from.
* 2677, when a `\⏎` pair is deleted. This one `goto restart_read`s *past* 2361,
  so a continuation costs exactly one line, not two.

A simple command is stamped with `line_number` as its first element reduces,
which is after one token of lookahead has been read — osh models that already
(`Parser::simple_command_line`), but from the lookahead token's recorded end
line, and that line stops at the token's last character rather than after the
continuation the scan consumed to find its end.

Separately, after the continuation bash's reader returns **EOF**, so `cat >\` at
end of input is `syntax error: unexpected end of file`; osh yields a `Newline`
token instead and reports `near unexpected token \`newline'` — and one line too
high, which is the same accounting in the other direction.

**Fix.** Two parts, landed together because either alone moves a line number the
other would move back:

1. A token's recorded end line now includes a `\⏎` the scan consumed *after* its
   last character (`Spans::cont_lines`, folded into `Parser::reader_line_at`) —
   the scan does consume it, it must, to discover the word has ended, so this is
   a stamping question, not a scanning one. On top of that sits bash's post-EOF
   bump: the parser's request for a lookahead that is not there costs a line.
   The bump is *not* unconditional, because `shell_ungetc` at
   `shell_input_line_index == 0` stows the character into `eol_ungetc_lookahead`,
   which the next `shell_getc` returns before fetching anything. So the request
   is free exactly when the last token peeked, was not read by `read_token_word`
   (that path does `if (character == EOF) goto got_token;` — parse.y:4904 — and
   pushes nothing back), and its peek deleted a continuation that ran to the end
   of the input. That is the `stowed` test in `reader_line_at`.
2. A continuation that ends the input now yields end-of-file rather than a
   newline token, so the grammar error is bash's. `Lexer::reader_at_eof` answers
   whether the scan stopped on a deleted `\⏎` (or the `\␍␊` a CRLF file writes),
   and `run_into` suppresses its synthetic trailing `Newline` when it did.
   `Lexer::fetched_line` adds the same post-EOF bump for the unterminated
   here-document warning, whose gather is driven by the `simple_list` reduction
   and therefore always pays for the end-of-file request.

Two smaller bugs fell out of this. `run_into`'s three terminal branches each
close with `stamp_lines`, which takes `self.line` as the line the iteration
*began* on — but `iter_start` was never moved to `self.pos` first, so
`cur_line()` added the previous iteration's newlines a second time and the
here-document warning came out at `2n+1` rather than `n+3`.

**Measured, not assumed.** Every number is bash 5.2.37's own output for the same
input, taken before the C source was read: 35 probe rows across
`$LINENO`, the here-document warning and the three end-of-file diagnostics, plus
a 224-case differential against the pre-change build (31 rows fixed, 0
regressed).

**Follow-up, same day: a request can cost more than one line.** The fix above
charged the end-of-file request a flat one fetch, which is right only when the
reader has nothing in front of it. A token bash completed *by* its own lookahead
— `&&`, `||`, `>&` — never looked again, so the whole trailing run of
continuations is still there and the request deletes them all, one
`line_number++` each (parse.y:2677):

```text
echo 1⏎echo a &&\⏎        line 3   (osh 3, agreed already)
echo 1⏎echo a &&\⏎\⏎      line 4   (osh was 3)
echo 1⏎echo a &&\⏎\⏎\⏎    line 5   (osh was 3)
echo 1⏎echo a 2>&\⏎\⏎     line 4   (osh was 3)
```

`Spans::pending_conts` counts that run and `reader_line_at` charges
`max(run, 1)` — never both a deletion and a fetch, since the fetch after a
deletion is the deletion's own `goto restart_read` and is not charged again.
Only a run that reaches the end of the input counts: if anything follows, that
free fetch brings the line in and its newline is the last token instead, which
is why `echo a &&\⏎␣␣⏎` was already right. Verified over 17 operator/compound
shapes × 4 tails — 0 diffs, from 6.

**Pinned by** the corpus case
`a-continuation-at-the-end-of-input-still-costs-the-reader-a-line.sh`, the new
unit test `a_continuation_at_end_of_input_moves_the_reader`, four new rows in
`simple_command_line_follows_bashs_lookahead_rule`, and six new rows in
`unterminated_heredoc_records_both_warning_lines`.

**Left behind**, all pre-existing and none of them regressions of this change —
see the three entries directly below:
TD-OILS-A-LIST-TERMINATOR-EOF-IS-REQUESTED-TWICE,
TD-OILS-A-STRING-READERS-BACKSLASH-CLOSE-DOES-NOT-COST-A-FETCH and
TD-OILS-AN-UNFINISHED-CONDITIONAL-AT-END-OF-INPUT-IS-NOT-A-MISSING-BRACKET.

**Deliberately not replicated:** bash's own reader has a bug next door. Reading
a here-document body from an `st_string` whose last line has no newline,
`read_a_line` stores `yy_string_get`'s `EOF` (-1) into a `char`, so the body
gains a stray `\xff`: `bash -c $'cat <<x\nbody\\'` writes `body\\\xff\n` where
the same bytes in a file write `body`. osh writes the file answer in both cases
and should keep doing so.
