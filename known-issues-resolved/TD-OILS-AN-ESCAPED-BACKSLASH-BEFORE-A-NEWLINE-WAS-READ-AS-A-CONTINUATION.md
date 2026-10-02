### TD-OILS-AN-ESCAPED-BACKSLASH-BEFORE-A-NEWLINE-WAS-READ-AS-A-CONTINUATION. An even run of backslashes ended a line one line too far down — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `ends_in_cont`, which
`Spans::reader_stop` consults for a span the lexer closed *past* a deleted
`\<newline>`.

**What.** `ends_in_cont` asked only whether the two characters before the span's
end were `\` and `\n`. It never looked at what stood in front of that backslash,
so `…\\⏎` — a *quoted* backslash and an ordinary newline — was charged a fetch
it never cost. Both readers, not just the string one:

```text
input (a script file)          bash                       osh
nosuch$LINENO\⏎                line 3                     line 3   (agrees)
nosuch$LINENO\\⏎               line 1                     line 2
nosuch$LINENO\\\⏎              line 3                     line 3   (agrees)
nosuch$LINENO\\\\⏎             line 1                     line 2
```

**Why.** `shell_getc` deletes `\<newline>` on sight, but `read_token_word` never
lets it see the character after a backslash: that one is read with
`shell_getc (0)`, continuation removal *off* (parse.y). So a run inside a word is
consumed in pairs, each pair yielding one literal backslash, and only an unpaired
last backslash is left to join with the newline. Parity decides, and the odd case
is the continuation.

The line only moves where the newline token is the one being placed, which for a
simple command is the lookahead of a *one-word* command: `simple_command_line`
takes bash's reduction of the command's first element, and that reduction waits
for one more token. `nosuch A\\⏎` was right all along because its lookahead is
the word `A\`, whose span ends before the newline.

Syntax errors hid it by cancelling: with `ends_in_cont` wrongly true the
end-of-file request was treated as `stowed` and charged nothing, and the two
mistakes came to the same number.

**Fixed.** `ends_in_cont` counts the run of backslashes ending at the one before
the newline and answers on its parity.

**Measured, not assumed.** bash 5.2.37 on one-line script files, runs of 1 to 4:
3, 1, 3, 1. Pinned by four new rows in
`simple_command_line_follows_bashs_lookahead_rule` and the corpus case
`a-backslash-before-a-newline-deletes-it-only-in-an-odd-run.sh`, which reads the
command's line back through a `DEBUG` trap — the only way to see it without
running a command whose name ends in a backslash, and what such a lookup *says*
is the host's business rather than the parser's. The corpus case fails on the
pre-fix build in five places, including two here-document delimiter lines.
