### TD-OILS-COND-ERROR-NEAR-IGNORES-THE-ALIAS-TEXT — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/lexer.rs` — `expand_aliases_tracked` and the new
`TokSpan` / `AliasBody` / `AliasExpansion`; `userspace/oils/src/parser.rs` —
`Spans` (now a *table* of texts), `Spans::reader_stop`, the new
`Spans::echo_line` / `Parser::reader_echo`, and `ParseError::echo`;
`userspace/oils/src/interp.rs` — `Shell::format_parse_error`.

**What.** When the offending text arrived through an alias, bash reported
against the *expansion* and osh against the line as written:

```sh
shopt -s expand_aliases
alias A="[[ P;Q"
A ]]
```

```text
bash: line 3: syntax error near `;Q'   line 3: `[[ P;Q'
osh:  line 3: syntax error near `;'    line 3: `A ]]'
```

**Why.** osh treated an alias splice as text that does not exist: the spliced
tokens were marked `u32::MAX`, `Spans::near` declined, and the diagnostic fell
back to naming the token and echoing the physical line. But the tokens *were*
written somewhere — in the alias's value — and bash reads that value by
pushing it onto the **input**: `push_string` (parse.y 1932) assigns
`shell_input_line = s` outright, and `pop_string` restores the line it
displaced once the reader passes the replacement's end. So while the reader is
inside a replacement, the current input line *is* the replacement, and that is
what `error_token_from_text` slices and what `report_syntax_error` echoes.

The pop is the whole of the rest of the rule, and it is observable: the same
`;` is blamed on two different texts depending only on whether anything
follows it inside the alias.

```text
alias A="[[ P;Q";  A ]]      near `;Q'   line `[[ P;Q'
alias A="[[ P ;";  A Q ]]    near `A'    line `A Q ]]'
```

In the second the `;` is the replacement's last character, so the look past it
is the read that finds the pushed string used up — and the error lands on the
written line, at the offset just past the alias word. An operator completed by
its own lookahead never makes that read, so it stays inside the replacement
even flush against its end (`alias A="[[ a>>"; A b ]]` → near `a>>`, line
`[[ a>>`).

**Fixed by** making a replacement a first-class *source*. `Spans` holds a table
of texts rather than one: `srcs[0]` is the shell's own input and the rest are
the replacements the alias pass pushed, each with the `parent` span
`pop_string` would restore. Every token carries a `TokSpan` — which text it
was read from and where in it it ends — so `Spans::near` runs bash's textual
scan over the right text, and `Spans::reader_stop` pops a replacement the
reader has read past before scanning. The echoed line follows the same walk
(`Spans::echo_line`), riding out on `ParseError::echo` for
`format_parse_error` to print in place of the physical line.

The line *number* is unchanged and still the alias word's: a replacement is not
a line of the script and has none of its own.

**Pinned by** `tests/corpus/an-alias-is-the-input-line-while-it-is-being-read.sh`
— 21 shapes, including both sides of the pop, the non-peeking operator, a
nested alias, and a capture.
