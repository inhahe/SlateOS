### TD-OILS-A-BACKSLASH-CLOSED-STRING-STILL-FINDS-A-TOKEN-TO-REPORT-NEAR. `[[ a == b )\` gets a `near` line bash does not print — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Parser::cond_operand_error` and the
`syntax error near` construction it shares with the rest of the parser
(`Parser::cond_near`, `Parser::token_display`), plus whatever decides that a
diagnostic gets an echoed source line.

**What.** The message *after* a `syntax error in conditional expression` is
chosen by which branch of `report_syntax_error` (parse.y:6232) the input reaches.
When the conditional's own error left the reader mid-line, bash finds the
offending text and prints `syntax error near` + the line; when the string was
closed with a backslash, the fetch that discovered the end left
`shell_input_line` empty, so bash falls past *both* `near` branches (the
`current_token` one at 6251 and the `error_token_from_text` one at 6273) to the
final `else`, and prints a bare `syntax error` with no line under it:

```text
echo 1⏎[[ a == b )⏎     both:  line 2: syntax error in conditional expression: unexpected token `)'
                               line 2: syntax error near `)'
                               line 2: `[[ a == b )'

echo 1⏎[[ a == b )\⏎    bash:  line 2: syntax error in conditional expression: unexpected token `)'
                               line 3: syntax error
                        osh:   line 2: syntax error in conditional expression: unexpected token `)'
                               line 3: syntax error near `)\'
                               line 3: `'
```

Two things are wrong on osh's side. It still produces a `near` line where bash
has no input line left to find a token in — and the token it names is `)\`, with
the closing backslash glued on, where the token is just `)`. (The empty echoed
line is *correct* on the branches that do echo — see the entry above — but this
branch does not echo at all.)

Note `EOF_Reached` is 0 here, which is why bash says `syntax error` and not
`syntax error: unexpected end of file`: the conditional died on the `)` before
the reader was asked for anything past the close.

**Fixed.** Every conditional `near` line now goes through
`Parser::cond_sequel_at`, which asks `Spans::reader_line_empty` whether the
reader still had a line when the offending token was finished. Empty, and the
message is a bare `syntax error` — which also drops the echo, since
`format_parse_error` keys the echo off the presence of `syntax error near `.

The emptiness test is `Spans::reader_stop`'s own walk: the fetch that empties the
buffer is the one it already charges a line for, so "the walk crossed something
(`n > 0`) and ran off the end of the text" names exactly the two ways to get
there — a `\<newline>` flush against the token with nothing behind it, and a
`-c` string `close_last_line` closed with a backslash.

The second half of the report was wrong, and the measurement corrected it. The
`near `)\'` osh printed is not a token with a stray backslash glued on: bash
prints exactly that for `bash -c '[[ a == b )\'`, and echoes ``[[ a == b )\\``
under it. The `\` really is part of the text there, because the close doubled it
and the reader stops *on* it. What made the original probe different was that its
`\` was a continuation, so the reader deleted it and found nothing behind — which
is the emptiness the fix now tests for, not the token text.

Which of the two a given input gets turns on the token rather than the input:
`bash -c '[[ a b\'` empties the buffer (the word's scan takes both backslashes
and runs it out) where `bash -c '[[ a == b )\'` does not. One space is the other
discriminator — `[[ a == b ) \` stops on the space and keeps its whole line,
trailing backslash and all.

The general (non-conditional) errors are untouched and were already right: they
leave through `report_syntax_error`'s *first* branch, whose `print_offending_line`
is unconditional (parse.y:6262), so `echo 1⏎;;\⏎` keeps both lines and simply
echoes an empty one.

**Pinned by** `parser.rs`'s `a_backslash_closed_string_leaves_no_line_to_report_near`
(14 rows: the five emptied forms including a group's surviving `expected `)'`, the
space and not-flush contrasts, a following line, the three general-error forms,
and the three `-c` string-closed forms) and the corpus case
`a-backslash-closed-string-leaves-no-line-to-report-near.sh`.

**Impact.** One or two extra diagnostic lines on an input that is a syntax error
either way, only when the string was closed with a backslash.
