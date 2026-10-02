### TD-OILS-A-REWOUND-ARITHMETIC-COMMAND-IS-NOT-REWOUND-THE-WAY-BASH-REWINDS-IT — 2026-08-05 — ⏳ MOSTLY FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs`, the rewind in the `'('` arm of the
tokenizer (see TD-OILS-AN-ARITHMETIC-COMMAND-NEEDS-ITS-CLOSERS-ADJACENT, which
is what put it there).

**What.** osh rewinds a non-adjacent `(( … ) )` by moving the cursor back and
re-lexing the *original source*. bash instead pushes a **reconstructed string**
— `(` + body + `)` + the one character its adjacency test read — into the input
and parses that. The two agree on everything that parses, and differed in four
places where something does not. **The fourth — the `for` header — is fixed as
of 2026-08-08** (see *Fixed: the `for` header* below); three remained:

```text
(( ) )                    bash: line 1: syntax error near unexpected token `)'
                                line 1: `( ) '
                          osh:  line 1: syntax error near unexpected token `)'
                                line 1: `(( ) )'

((1+1)<nl>)               under `bash -c`, `eval`, `.`:
                                line 1: syntax error near `((1+1)'
                                line 1: `((1+1)'
                          under `bash file` or `bash < file`:
                                line 1: 1+1: command not found
                          osh:  line 1: 1+1: command not found   (either way)

((1+1)\<nl>) && echo hit  bash: line 2: syntax error near unexpected token `'
                                line 2: `) && echo hit'
                          osh:  line 2: 1+1: command not found
```

**The first of those, and the two further families measured below, are fixed as
of 2026-08-10** — see *Fixed: the copy is a real substitute input buffer*. What
is left is the continuation row, the string-vs-stream row, and one more found
while fixing the rest (the exit status a newline-terminated copy leaves behind);
all three are listed under *Still open* at the end.

**Why each one.**

*The echoed line.* `print_offending_line` echoes `shell_input_line`, and while
the parser is inside the pushed string that *is* `shell_input_line`. So `(( ) )`
echoes `( ) ` — the reconstruction — not the line as written. osh has no such
substitute buffer and echoes the real line.

*The newline row, and why bash disagrees with itself.* Reading `((1+1)<nl>)`
from a **stream** (an executed script, or stdin) bash runs it, exactly as osh
does. Reading the identical text from a **string** — `-c`, `eval`, `source` —
bash reports a syntax error instead. The difference is in how `push_string`'s
saved line interacts with a string input source, not in the parse itself, and it
makes bash's own two answers to the same input disagree. osh follows the stream
answer, which is the one that is not a self-contradiction.

*The continuation row.* bash's rewind is visibly lossy here: the reader has
already deleted the `\<newline>`, and reconstructing cannot put it back, so the
re-parse desynchronises and reports `unexpected token `'` — with an **empty**
token name — on the line the second `)` ended up on. osh's rewind loses nothing
and simply runs the subshells.

**Fixed: the `for` header** — 2026-08-08. Failing the adjacency test in a `for`
header makes `parse_dparen` return −1 (parse.y:4478), which `read_token` hands
to bison as a value ≤ 0 — i.e. as the EOF token, though `EOF_Reached` is not
set. bash's own `current_token` is that −1, for which `error_token_from_token`
has no branch and returns NULL, so `report_syntax_error` falls past its naming
branch (parse.y:6251) into the *text-scanning* one (6276): `syntax error near
\`X'` with `X` sliced by `error_token_from_text` around `shell_input_line_index`
— parked one past the single character the test read — and the offending line
echoed underneath. osh used to raise a bare `LexError` reading `malformed
arithmetic expansion`, with no slice and no echoed line.

Modelled with a new **`Tok::Refused`**: a refusal with no name, which
`Parser::error_token_at` answers `None` for (as it already did for `VarFd`, the
other token bash's switch declines to name), sending `unexpected_here` down the
`near_at` path that was already a faithful port of `error_token_from_text`. The
lexer emits it in place of the error, consuming the one character the adjacency
test read so the token's recorded end *is* `shell_input_line_index`, and then
stops the scan — bash's reader never fetches another line, because bison has
already errored. `Reader::of` gives it `peeks: false`: the look past the
construct is the `shell_getc (0)` already counted in its span.

One correction to `Spans::near` came with it. An offset sitting **one past a
newline** is ambiguous in a flat buffer and was always resolved as the start of
the next line, but bash's `shell_input_line` holds *one* line plus its NUL, so
an index of `strlen` is still that line — which is exactly why
`error_token_from_text`'s first test steps back onto the newline. Only a *fetch*
replaces the buffer, and `reader_stop` already counts those, so the count now
settles it: `for ((i=0;i<1;i++)⏎)` is reported near `;i++)` against line 1,
while a deleted `\⏎` — which did fetch — still reports against line 2.

Pinned by `tests/corpus/an-arithmetic-for-header-that-fails-the-adjacency-test-is-blamed-by-position.sh`
and `parser::tests::a_for_header_that_fails_the_adjacency_test_is_blamed_by_position`.

**Two more families, measured 2026-08-10.** The three rows above are all
*syntax errors*, which made the divergence look message-only. It is not. The
same substitute buffer decides what line every command inside the re-read text
is blamed on, and what line the end of input is blamed on.

*Every line inside the copy collapses onto the line the scan ended on.*
`push_string` hands the reader a string, and `line_number` is neither rewound
before the push nor advanced by the newlines inside it — it stands wherever
`parse_arith_cmd`'s scan left it for the whole re-read:

```text
(( (nosuch⏎) ) )              bash: line 2: nosuch: command not found
                              osh:  line 1: nosuch: command not found
(( (nosuch⏎⏎) ) )             bash: line 3: …          osh: line 1: …
(( (echo A=$LINENO⏎) ) )      bash: A=2                osh: A=1
(( (1 ⏎)) )                   bash: line 2: 1: command not found
                              osh:  line 1: …
f() { (( (nosuch⏎) ) ); }; f  bash: line 2: …          osh: line 1: …
```

The plain spelling `( (nosuch⏎) )` — no `((`, so no push — reports line 1 in
both, which isolates the cause. `$LINENO` *after* the construct is right in
both: the counter resyncs on the next real fetch.

*At end of input the line is floored at the scan's last line plus two.* The copy
ends with the newline it took as its one tested character, so that newline is
handed over twice and the reader asks for a line once more than it otherwise
would. Where a real line answers the extra ask nothing shows; at end of input it
costs a line:

| script | scan ends on | bash | osh |
|---|---|---|---|
| `(( (1 ))` (no final newline) | 1 | 3 | 2 |
| `(( (1 ))⏎` | 1 | 3 | 2 |
| `(( (1 ))⏎⏎` | 1 | 3 | 3 |
| `(( (1 ))⏎⏎⏎` | 1 | 4 | 4 |
| `(( (1⏎))⏎` | 2 | 4 | 3 |
| `(( (1⏎))⏎⏎` | 2 | 4 | 4 |
| `(( (1⏎⏎))⏎` | 3 | 5 | 4 |

so bash reports `max(normal, scan_end_line + 2)`. The charge is owed only when
the tested character was a newline: `(( (1 ));`, `(( (1 )) `, `(( (1 ))&` and
`(( (1 ));` with no final newline all agree with osh. The plain `( (1 )⏎` agrees
too, which again isolates the push as the cause.

*The echoed-line row generalises, and the pushes stack.* The reconstruction is
always exactly `src[start+1 ..= scan_end]` — the physical text with the first
`(` dropped, run through the character that failed the adjacency test — so no
text has to be synthesised to model it, only re-labelled:

```text
(( 1 + (2 ))       bash echoes `( 1 + (2 ))'     osh: `(( 1 + (2 ))'
(( fi ) )          bash echoes `( fi ) '         osh: `(( fi ) )'
(( 1; done ) )     bash echoes `( 1; done ) '    osh: `(( 1; done ) )'
(( 1⏎+ (2 ))       bash echoes `( 1⏎+ (2 ))'     osh: `+ (2 ))'
```

Note `(( fi ) )` → `( fi ) `: the final `)` is not in the copy — it is the
tested character that follows the space — and the multi-line row shows the copy
is echoed whole, embedded newline and all, where osh echoes one physical line. A
token read *after* the copy is exhausted is echoed from the physical line again
and the two agree (`(( (1 )) fi`, `(( (1 ))x`). And the pushes **stack**, as
`push_string`'s list does: `(( (( fi ) ) ) )` pushes `( (( fi ) ) ) `, whose
re-read meets `((` again and pushes `( fi ) ` on top of it — and that innermost
copy is what bash echoes.

**Impact.** Not cosmetic, as first recorded. The syntax-error rows are shapes
bash also rejects, but the two line-number families are ordinary runtime
diagnostics on scripts that run to completion.

**Fixed: the copy is a real substitute input buffer** — 2026-08-10. The cursor
is still rewound — re-reading the physical text is what makes the copy's
*content* right without synthesising anything, since the reconstruction is
always exactly `src[start+1 ..= scan_end]` — but the region is now *recorded*,
so everything that reads off `shell_input_line` can be answered from it.

- `Lexer::dparens` (`lexer.rs`) collects a `DparenPush { start, end, line,
  eof_charge }` at the `(None, _)` arm of the `'('` case: the copy's extent, the
  line `line_number` was parked on, and whether the tested character was the last
  of its input line.
- `Lexer::stamp_lines` raises a token's stamped line to the enclosing push's
  `line` — the *innermost* containing push, since a nested copy is read while the
  enclosing one is still frozen. `self.line` keeps advancing underneath, so the
  resync after the copy needs no separate step.
- `Spans::push_dparens` (`parser.rs`) adds each copy to `Spans::srcs` and moves
  the tokens read out of it onto that text, with `parents` pointing just past the
  tested character — for a nested push, at an offset *inside the enclosing copy*,
  which is what makes `reader_stop` unwind them in `push_string`'s own order.
  `Spans::echo_line` already returned the whole text for any `src != 0`; it now
  also strips trailing newlines, as `print_offending_line` does.
- `Tokenized::dparen_eof_floor` carries `scan_end + 2` for every copy that ended
  on a consumed newline, and `Parser::reader_line_at` floors its end-of-input
  answer at it — beside the `bump` `eof_closed_in_list` already had.
- `Spans::echo_line_at_scan` unwinds the copies again for the one error the
  *scan* raises. bash parses each `$( … )` body inside the arithmetic while
  still looking for the closing `))` (`parse_matched_pair` →
  `extract_command_subst`), so a body that does not parse is reported before
  `parse_arith_cmd` has decided anything and `shell_input_line` is still the
  physical line: `(( $(fi ))` echoes `(( $(fi ))`, not the copy `( $(fi ))`.
  osh lowers those bodies later, while the copy is being re-read, so the two
  central stamping sites ask for the pre-push echo whenever the error already
  names a line — which is exactly an error raised in a `$( … )` body of its own.
- `Lexer::take_nul_word` emits the word the popped buffer's NUL starts. A copy
  whose last character was its line's newline is exhausted with the reader's
  saved index on that NUL, which `shell_getc` hands back as input and
  `read_token_word` takes for the start of a word — so the token buffer opens
  with a NUL and the word's *value* is `""` however much text is appended after
  it. The run up to the next delimiter is therefore read and lost
  (`((:)<nl>echo A)` runs `A`), and the empty unquoted word is removed by
  splitting, so a copy followed by nothing but its `)` leaves a command with no
  words and status 0. osh reads that run as the word it is and writes a
  `Seg::Lit(b"\0")` in front of it, which is what bash's buffer holds; the cut
  is then the ordinary `make_word` one `word_expanded_from_its_text` already
  models. Keeping the segments is what keeps a `$( … )` in the run *parsed*
  where the scan meets it — `((:)<nl>$(fi))` is fatal — while the cut is what
  keeps it from being performed.

Pinned by `tests/corpus/a-rewound-arithmetic-command-is-re-read-from-a-copy.sh`.

**Still open.**

*A copy pushed inside an alias replacement is echoed as the physical line.* The
alias pass re-lexes spliced text and re-labels its offsets through its own
`TextMap` into the alias source table, so a push recorded against the *spliced*
text cannot be named in that table's terms and is dropped
(`lexer.rs`, the `Spanned { … dparens: _ }` in the alias splice). The line
freeze still applies, since the lex that records the push is the one that stamps
the lines; only the echoed text is affected:

```text
shopt -s expand_aliases; alias a='(( fi ) )'; a
    bash: line 3: `( fi ) '        osh: line 3: `(( fi ) )'
```

*The continuation row.* bash's rebuild is lossy where the reader had already
deleted a `\<newline>`: the copy cannot put it back, so bash's own re-parse
desynchronises and reports `unexpected token `'` — with an **empty** token name
— on the line the second `)` ended up on. Reproducing it needs the *content* of
the copy to be synthesised rather than re-read, which is the one thing the
recorded-region model does not do.

*The string-vs-stream row.* `bash -c`, `eval` and `.` still disagree with `bash
file` and `bash < file` whenever the copy ends on a newline *at end of input*:
the string reader stops with the physical line still current and its index past
the tested newline, so the diagnostic is `syntax error near \`X'` against the
scan's last line with that physical line echoed, rather than `unexpected end of
file` at `scan_end + 2`:

```text
                              bash -c / eval / .          bash file / bash < file
(( (1 ))                      line 1: near `))'           line 3: unexpected EOF
                                      `(( (1 ))'
(( (1⏎))⏎                     line 2: near `))'           line 4: unexpected EOF
                                      `))'
(( (1 ) ) ; (( (2 ) )         line 1: near `)'            line 3: unexpected EOF
(( (1 ));                     line 2: unexpected EOF      line 2: unexpected EOF
```

The last row is the control: a copy ending on `;` agrees, so the split is
exactly the newline-terminated copy. osh follows the stream answer, which is the
one that is not a self-contradiction.
