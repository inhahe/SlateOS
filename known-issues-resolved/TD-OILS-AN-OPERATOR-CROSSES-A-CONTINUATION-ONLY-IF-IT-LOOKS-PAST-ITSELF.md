### TD-OILS-AN-OPERATOR-CROSSES-A-CONTINUATION-ONLY-IF-IT-LOOKS-PAST-ITSELF — 2026-08-05 — ✅ FIXED 2026-08-05

*(was TD-OILS-A-ONE-CHARACTER-REDIRECTION-OPERATOR-PEEKS-PAST-A-CONTINUATION,
which named only the shape that reported too **early**; measuring the whole
operator table turned up an opposite set that reported too **late**, and the two
turned out to be one rule seen from both sides.)*

**Where:** `userspace/oils/src/parser.rs` — `Spans::reader_stop`, and the
`Reader` descriptor that feeds it.

**What.** A `\<newline>` moves a syntax error's line only when bash's reader
actually reached it, and osh moved it for every token:

```text
[[ 2>\           bash: line 2: unexpected token 284 in conditional command
Q ]]                   line 2: syntax error near `Q'
                       line 2: `Q ]]'
                 was:  line 1: … near `2>' … `[[ 2>\'

[[ a>>\          bash: line 1: unexpected token `>>', conditional binary …
Q ]]                   line 1: syntax error near `a>>\'
                       line 1: `[[ a>>\'
                 was:  line 2: … near `Q' … `Q ]]'
```

`[[ {fd}>\<nl>Q ]]` and `[[ {fd}<\<nl>Q ]]` (token 283, REDIR_WORD) went the
first way; `[[ a>&\<nl>Q ]]`, `[[ >>\<nl>Q ]]` and `[[ 2&>>\<nl>Q ]]` the
second. The second class was a regression from
TD-OILS-COND-ERROR-LINE-AFTER-A-CONTINUATION's fix, which moved the line for
*every* token.

**Why.** bash's `read_token` reads one character and, for a shell
metacharacter, immediately takes `peek_char = shell_getc (1)` — a read *with*
continuation removal. Where that peek completes a longer operator the operator
is returned right there, and the reader stops on the character after it. Where
it does not, the peek is pushed back with `shell_ungetc` — but the continuation
it deleted on the way is gone, and `line_number` has already moved. So an
operator crosses a flush continuation **unless it is a multi-character operator
its own lookahead completed**:

| crosses | does not |
|---|---|
| `>` `<` `;` `&` `\|` `(` `)` — the peek was pushed back | `>>` `>&` `<&` `<>` `>\|` `\|&` `;&` `&&` `\|\|` `<<-` `<<<` `&>>` `;;&` |
| `<<` `;;` `&>` — each peeks once *more* (for `<<-`/`<<<`, for `;;&`, for `&>>`) and pushes that peek back | |

A word reaches one character further still, and that is the first class. The
cause is not the operator's disambiguation peek — if it were, `[[ 2>&\<nl>Q ]]`
would diverge too, and it does not. It is `read_token_word`: having read the
terminator that ends the word it tests `shellexp (character)`, true for `<` and
`>`, and peeks once more for a `<( … )` process substitution. That peek deletes
a continuation written after the `<`/`>` even though the word then pushes *both*
characters back — and `shell_ungetc` cannot push past the start of the line it
has just fetched, so the reader is left at the top of it. Hence line 2 and a
slice of `Q`, where the same NUMBER token in `[[ 2>Q ]]` gives line 1 and `2>`.

**Fixed by** replacing the unconditional continuation walk with
`Spans::reader_stop`, which answers *where the reader stopped* and *how many
lines it fetched* together, from a `Reader { peeks, word }` descriptor built off
the token. `peeks` is false for exactly the thirteen operators above and
suppresses the walk — which also keeps the backslash in `Spans::near`'s slice,
as bash's does (`a>>\`). `word` marks the tokens `read_token_word` produces and
enables the one-character-further step when the character at the span's end is
`<` or `>` and a continuation follows it.

**Pinned by**
`tests/corpus/an-operator-crosses-a-continuation-only-if-it-looks-past-itself.sh`
— the whole table, both directions, inside a conditional and outside it.
