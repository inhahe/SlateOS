### TD-OILS-A-RESERVED-WORDS-SPELLING-IS-NOT-ITS-CLASSIFICATION. `echo do ((1))` and `[[ a ]] ((1))` both name the wrong token — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/lexer.rs` — `arith_cmd_position` and the
`ARITH_CMD_AFTER` list it consults, called from the tokenizer's `'('` arm to
decide whether a flush `((` opens an arithmetic command.

**What.** osh decides from the previous token's *spelling*. bash decides from
`last_read_token`, which is the previous token's *classification* — and a word
is classified as a reserved word only where one was already acceptable
(`CHECK_FOR_RESERVED_WORD`, parse.y:2994–2997, is itself gated on
`reserved_word_acceptable (last_read_token)`). The rule is therefore recursive,
and a spelling table cannot express it. Two halves fall out.

**Over-trigger.** A word that merely *looks* like a reserved word is treated as
one. In bash `echo do` reads `do` as a plain WORD, so `((` after it is not
arithmetic and the error names the `(`; osh makes it arithmetic and names the
expression:

```text
echo do ((1))       bash: syntax error near unexpected token `('
                    osh:  syntax error near unexpected token `word'
```

Measured the same way for `done`, `fi`, `then`, `!`, `time`, `{`, `esac` and
`until`. (`x=do ((1))` already agrees — the assignment prefix blocks it by a
route osh has.)

**Under-trigger.** `]]` is `COND_END`, which *is* on bash's acceptable list, but
is absent from `ARITH_CMD_AFTER`. bash reads the `((` as arithmetic and the
error names the expression it collected; osh hands back a bare `(`:

```text
[[ a ]] ((1))       bash: syntax error near unexpected token `1'
                    osh:  syntax error near unexpected token `('
```

`]]` has to be the conditional's closer, not a word spelled `]]` — `echo ]] ((1))`
names `(` in both, and would start naming `1` if the list simply gained the
string. The lexer already tracks `cond_depth`, which is what tells them apart.

**Impact.** The wrong token named in a syntax error, in both directions. Not
purely diagnostic: see the sibling entry below, where the same `function` special
case is the difference between a working function definition and a parse failure.

**Fixed.** The spelling table is gone, replaced by bash's own state machine.
`RwAccept` (lexer.rs) is a fold over the emitted token stream carrying the one
bit `reserved_word_acceptable` would compute: set by a newline, by each operator
on bash's list (`; ( ) | & && || ;; ;& ;;& |&`) and by an `ARITH_CMD`; set after a
*word* only when that word was itself classified as one of the reserved words
that leave a reserved word acceptable (`RW_LEAVES_ACCEPTABLE`); and cleared after
everything else. Classification is where the recursion lives: a word is a
reserved word only if the flag was already set when it arrived, which is exactly
why `echo do` leaves `do` an ordinary word. bash's two lookbehinds are one extra
field (`RwPrev`), so a plain WORD after `function` or after `coproc` is
acceptable but the word after *that* is not. `arith_cmd_position` now advances
the fold over any tokens emitted since it last looked and reads the bit, instead
of re-deciding from text.

The `[[ … ]]` half is a *freeze*, not a bracket count: `parse_cond_command` runs
from inside a single `read_token` (parse.y:3399), so `last_read_token` never
moves for the length of a conditional and comes back as `COND_END`. `RwAccept`
holds its own `cond` flag for this and deliberately does not reuse the lexer's
`cond_depth`, which tracks a lexing mode and so counts every bare `[[` —
including the ones bash would never have classified as `COND_START`.

The corpus run then forced out a follow-on: with `[[ a ]] ((1))` finally taking
the arithmetic path, osh named it `word`. `error_token_from_token` returns
`string_list (yylval.word_list)` for an `ARITH_CMD`, i.e. the expression it
collected — verbatim, inner spaces and all — so `token_display_at` gained a
`Tok::ArithCmd` arm that re-prints the body the same way the parser does
(`splice_reprints(parse_arith_comsubs(…))`, falling back to the raw text). A `((`
that stayed two parens is still named `(`, which is what makes the name double as
proof of which path was taken.

**Pinned by** the lexer test
`a_reserved_words_spelling_is_not_its_classification`, the parser test
`an_arithmetic_command_is_named_by_its_expression`, and the corpus case
`a-reserved-words-spelling-is-not-its-classification.sh`.
