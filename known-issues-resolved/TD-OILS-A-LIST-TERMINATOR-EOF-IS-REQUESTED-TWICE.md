### TD-OILS-A-LIST-TERMINATOR-EOF-IS-REQUESTED-TWICE. `for`/`select` cut off by a continuation report one line low — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Parser::parse_in_list`, where the
header's terminator is consumed, and the end-of-stream branch of
`Parser::reader_line_at` that stamps the error.

**What.** Only `for` and `select`, and only when a `\<newline>` runs the input
out right after the word list:

```text
input (a script file)          bash                       osh
echo 1⏎for i in a\⏎            line 5: syntax error: …    line 4
echo 1⏎for i in a\⏎\⏎          line 6                     line 5
echo 1⏎select v in a\⏎         line 5                     line 4
echo 1⏎for i in a⏎             line 3                     line 3   (agrees)
echo 1⏎while true\⏎            line 4                     line 4   (agrees)
```

**Why.** bash's rule is `for` `WORD` `newline_list` `in` `word_list`
**`list_terminator`** `newline_list` `do_group`, and `list_terminator` is
`'\n' | ';' | yacc_EOF` — the end-of-file token is a terminator in its own
right. So with the newline deleted, the EOF the word `a`'s peek produced is
*consumed* by `list_terminator`, and the `newline_list` after it has to ask for
another token. That request finds the buffer used up, enters `shell_getc`'s
fetch block and pays `line_number++` a second time (parse.y:2361). Every other
compound (`while`, `until`, `case`, `{ … }`, a function body) reaches its EOF
through a rule that does not accept one, so it asks only once.

`read_token_word` does `if (character == EOF) goto got_token;` (parse.y:4904),
which is why the first EOF is not pushed back and has to be re-requested at all.

**Fixed.** The end-of-stream bump in `reader_line_at` is one fetch because one
token was asked for; the site that consumes the terminator now says when two
were. `Parser::parse_in_list` sets `Parser::eof_closed_in_list` when
`skip_separators` had nothing to skip *and* nothing was left behind it — which
is exactly "the terminator was the end of input" — and the end-of-stream branch
of `reader_line_at` adds that one extra fetch on top of whatever the run of
continuations already cost. The flag lives on the parser rather than inside
`reader_line_at`, which has no idea which rule is reducing.

The two limits are bash's own and both are measured: only the `IN` forms carry a
`list_terminator`, so `for i\⏎` is charged once like anything else (bash 4, and
osh 4 before and after); and a real `;` or newline takes the terminator's place,
leaving the end of file still to be fetched by the `newline_list` — one request,
so `for i in a;\⏎` stays at 3. Nesting is irrelevant, as it should be: it is the
rule being reduced that asks twice.

**Measured, not assumed.** A 17-header × 6-tail script-file sweep (102 cases)
went from 28 divergences to 0, and an 11-header × 6-tail `-c` sweep left only the
lone-trailing-backslash rows, which are
`TD-OILS-A-STRING-READERS-BACKSLASH-CLOSE-DOES-NOT-COST-A-FETCH` below and not
this. Pinned by `a_for_lists_terminator_can_be_the_end_of_input_itself`
(parser.rs) and the corpus case
`a-for-lists-terminator-can-be-the-end-of-input-itself.sh`.
