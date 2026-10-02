### TD-OILS-A-DUP-WORDS-TRAILING-DASH-WAS-JUDGED-AFTER-QUOTE-REMOVAL-INSTEAD-OF-ON-THE-RAW-BYTE. `>&x\-` was not recognised as a move, and `>&$v--` was wrongly refused as one — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/ast.rs` — `dup_move_source`; and
`userspace/oils/src/unparse.rs` — `quoted_lit_src`.

**What.** Whether `>&word` is a *move* is settled by the parser on the word's
**raw last byte**, before any quote has been removed. `make_redirection`
(`make_cmd.c`), whose own comment on the test is `/* Yuck */`:

```c
w = dest_and_filename.filename;
wlen = strlen (w->word) - 1;
if (w->word[wlen] == '-')		/* Yuck */
  { w->word[wlen] = '\0';
    if (all_digits (w->word) && legal_number (w->word, &lfd) && lfd == (int)lfd)
      → r_move_output;        /* source is a plain number */
    else
      → r_move_output_word; } /* …or a word still to be expanded */
```

osh instead tested the last *`WordPart`*, after the lexer had already turned
`\-` into a quoted part and after its own leading-dash guard. Two divergences
fell out, in opposite directions:

| spelling | bash | osh (before) |
|---|---|---|
| `1>&\-` | move, source expands to nothing → `cannot duplicate fd` + `1: Bad file descriptor`, rc=1 | close of fd 1, **rc=0** |
| `1>&x\-` | move from `x` → `x: ambiguous redirect`, rc=1 | dup word `x-` |
| `1>&$v--`, `1>&"x"--`, `1>&""--` | move, source `$v-` / `"x"-` / `""-` | **not** a move |
| `1>&3"-"`, `1>&3'-'` | not a move (raw last byte is the quote) | not a move ✓ |

`declare -f` is the witness, because the printer spells each instruction
differently: a close is always `%d>&-`, a move `%d>&%s-`, and a plain dup word
`>&%s` with the redirector *omitted* when it is the default. So bash prints
`true 1>&x\-` where osh printed `true >&x\-`.

**Why osh had a leading-dash guard at all.** It was standing in for the lexer
rule above — with `>&--` arriving as one word, `dup_move_source` had to decline
it by hand. Once the lexer took that over the guard became not merely dead but
*wrong*, because bash has no exception for an already-doubled dash: only the
final byte comes off, so `>&x--` is a move whose source is `x-`.

**Fixed** by matching bash's test rather than approximating it:

* `dup_move_source` now also accepts a last part that is a backslash-escaped
  run ending in `-`, since a backslash leaves the dash last after all. Taking
  the byte off leaves the backslash *dangling*, and bash expands that as the
  source — quote removal drops it, so the source is empty. osh represents it as
  an escaped run with nothing in it, which expands to the empty string.
* The leading-dash guard is gone; the `parts.is_empty()` one stays only as a
  can't-happen belt (the lexer guarantees a bare `-` never arrives as a word)
  so that a `Word` with no parts cannot escape.
* `quoted_lit_src` prints an empty escaped run as a single `\`, which is the
  only way that part is ever built and exactly what bash echoes back, so
  `1>&x\-` reads back as written.

Nineteen spellings of the target were compared end to end — `declare -f`
rendering, exit status, and diagnostics — and all nineteen now agree. Covered
by `tests/corpus/a-dup-targets-trailing-dash-is-judged-by-its-raw-last-byte.sh`
and by unit tests in `ast.rs`; one existing assertion there
(`>&3\-` → `DupSpelling::Word`) had encoded the bug and is corrected to
`MoveWord`.
