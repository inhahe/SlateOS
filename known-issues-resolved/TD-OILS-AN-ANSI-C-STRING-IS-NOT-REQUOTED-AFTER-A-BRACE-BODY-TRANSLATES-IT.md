### TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT. `"${h[$'a]b']}"` is a bad substitution in bash — 2026-08-07 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/lexer.rs` — `read_dollar_brace_body`, whose `'`
arm copies a `$'…'` through unchanged when the enclosing scan is double-quoted
(the `in_dquote` branch). The *unquoted* case is done — it translates and
re-quotes; see
`TD-OILS-A-DOLLAR-QUOTE-IN-AN-ARITHMETIC-STRING-IS-NOT-TRANSLATED-AT-PARSE-TIME`
and the corpus case
`an-ansi-c-string-in-a-brace-body-is-translated-at-parse-time.sh`. What is left
here is only the fourth branch: translate *without* re-quoting.

**What.** bash does not carry a `$'…'` through a `${ … }` body as text. It
**translates** it where it reads it — `ansiexpand`, parse.y:3854, backing the
write index up over the `$'` — and then decides whether to put single quotes
back around the result. That decision is a three-way one, and in one of the
three the quotes are *not* restored, so a `]` or a `'` that the source had
protected is suddenly bare:

| where the `$'…'` sits | requoted? | parse.y |
|---|---|---|
| word is unquoted (`(rflags & P_DQUOTE) == 0`) | yes, `sh_single_quote` | 3881 |
| in double quotes, after an operator — `dolbrace_state` is `DOLBRACE_QUOTE`/`QUOTE2`, i.e. past a `#`, `%`, `/`, `^` or `,` | yes, `sh_single_quote` (compat > 42) | 3865 |
| in double quotes, still in the parameter part or in a `:-`-style word — `DOLBRACE_PARAM` / `DOLBRACE_WORD` | **no**, spliced bare | 3875, inside `#if 0 /* TAG:bash-5.3 */` |

Measured (`h` is an associative array with an `a]b` key, `q="za'bz"`):

```text
                              bash                              osh
"${h[$'a]b']}"    ${h[a]b]}: bad substitution                    W
${h[$'a]b']}      W                                              W
"${q:-$'a\'b'}"   no closing `}' in "${q:-a'b}"                   za'bz
"${q/$'a\'b'/Y}"  zYz                                            zYz
```

Rows 2 and 4 are the requoted cases and already match. Rows 1 and 3 are the
`#if 0` hole: the translated text is spliced bare, the `]` closes the subscript
early and the `'` swallows the `}`, and bash reports a `bad substitution` that
quotes the *translated* text back — which is how the mechanism is visible at
all.

More of the same hole, measured 2026-08-07 while doing the unquoted case (`x`
unset throughout):

```text
                    bash                                       osh
"${x:-$'a\0b'}"     no closing `}' in "${x:-a          a
"${x:-$'\x27'}"     no closing `}' in "${x:-           '
"${x:-$'a}b'}"      ab}                                a}b
"${x:-$'a\'b'}"     no closing `}' in "${x:-a'b}"      a'b
```

Row 3 is the sharpest: the bare `}` in the translation closes the brace where
the source had it quoted, so bash's word is `a` followed by a literal `b}`.
Rows 1, 2 and 4 all end the same way — the lone `'` the translation contains
opens a quote that eats the closing brace.

The **splitting** shape is the same defect wearing different clothes:

```text
                                bash              osh
"$(echo ${x:-$'a\tb'})"         a b               a<TAB>b
```

The `$( … )` is inside double quotes, so bash's `dstack` still holds the `"`
when `read_token_word` reaches the `${`; `cd == '"'` makes `rflags` `P_DQUOTE`
(parse.y:3696) and the body takes branch 4. The translated tab is spliced
unquoted into the command that the `$( … )` re-parses, so it splits — and a
`$'a*b'` there would glob. Written *outside* quotes, or inside `$(( … ))` (where
`parse_comsub` drops the incoming flags for bare `P_ARITH`, parse.y:4083), the
same text is re-quoted and does neither.

**The disposition, settled.** The `#if 0` block is tagged `TAG:bash-5.3`: bash
has the fix written and *disabled*. Under §105 of `design-decisions.md` that is
the opposite of a waivable divergence — an unchecked error path may be waived,
but a guard deliberately keeping the 5.2 answer is behaviour, and behaviour gets
matched. So this was implemented rather than waived.

**Fixed in two stages.**

*Stage A — the re-quote decision.* `read_dollar_brace_body` now translates the
run in the `in_dquote` case too, and picks between `sh_single_quote` and a bare
splice on bash's own `dolbrace_state`, which `wordscan.rs` models as
`DolBrace` + `DolBrace::step` (parse.y:3809–3828, the same machine subst.c:1957
runs — "This logic must agree", parse.y:3803). Two details the C makes easy to
misread: the character is appended **before** the machine runs (parse.y:3785 →
3809), so `retind > 1` means "not the body's first character" and a one-letter
name like `"${q#$'z'}"` still reaches `QUOTE`; and a nested `${` **recurses**
(parse.y:3928) into a fresh `PARAM` while inheriting `P_DQUOTE`. A nested `$(`
strips `P_DQUOTE` from the flags it passes down (parse.y:3960) and that turns
out to change nothing — see
`TD-OILS-A-BRACE-BODY-IN-A-QUOTED-COMMAND-SUBSTITUTION-DOES-NOT-INHERIT-THE-QUOTE`
below.

*Stage B — what the bare splice then costs.* Splicing bare puts text into the
body that the scan never reads back, so the body can reach past the `}` the
*expansion* will stop at. bash gets this for free because **it reads a word
twice**: `parse_matched_pair` only looks for the end of the *word* and never
re-reads what it accumulated, so a `}` the translation contributed terminates
nothing there; `parameter_brace_expand` (subst.c:9539) then scans the finished
word text fresh, meets that `}` like any other, and closes on it — leaving the
rest as ordinary word text. Modelled as:

- `wordscan::expansion_body_len(body, quoted) -> BraceEnd` — runs the
  *expansion's* scan over `${body}` and answers `Same`, `Early(len)` (the
  expansion closes at `len`; `body[len+1..]` plus the parser's `}` is word text)
  or `Unclosed` (a spliced quote swallowed the `}`).
- `Lexer::bare_splices`, the ranges of the body `read_dollar_brace_body` wrote on
  the third row and never read back, which `read_dollar_brace` scopes to one
  `${ … }` (taken going in, restored coming out) and `shift_ranges` moves
  whenever a buffer is spliced into a longer one. They land as the fourth field
  of `Seg::ParamBraced`. A splice at any depth inside is that segment's, because
  it is that segment's raw text a leftover has to be carved out of. (Started as a
  `bare_splice` **flag**; it became a list when the operand needed to tell the
  spliced bytes from the read ones — see the `$' … '` splice paragraph under
  TD-OILS-A-SINGLE-QUOTED-COMMAND-SUBSTITUTION-IN-A-BRACE-OPERAND-IS-PARSED.)
- `parser::seg_to_parts` — one segment, one part, except where the flag is up and
  `expansion_body_len` says anything but `Same`. Then nothing about the body's
  *shape* can be asked (`"${x:-$'a"b'}"` is the word `"${x:-a"b}"`, whose `"`
  bash's parser never re-read either), so the body is deferred as
  `WordPart::BadSubst` text — and `word_from_segs_in` hands the **whole word**
  back as `WordPart::TokenText`, for `Shell::expander_word` to re-read at
  expansion time. See
  `TD-OILS-A-DOUBLE-QUOTE-IN-A-BARE-SPLICE-DOES-NOT-CLOSE-THE-ENCLOSING-RUN`
  below for why the leftover cannot be carved out of the segment instead.

Divergence 4 of the first table (`"${h[$'a]b']}"`) fell out of stage A on its own
— the bare `]` closes the subscript early, which the re-read scan already models.

Corpus case:
`userspace/oils/tests/corpus/an-ansi-c-string-in-a-double-quoted-brace-body-is-spliced-bare.sh`.

**Two rows were split out below**, because neither is about the re-quote
decision. `TD-OILS-A-NUL-IN-AN-ANSI-C-STRING-DOES-NOT-TRUNCATE-THE-TRANSLATION`
(row 1 of the second table) is now fixed — and turned out to be a *word*-scoped
cut at the token boundary rather than the splice-local truncation that entry
predicted, which is why fixing it also pulled in the `$( … )` second read and the
compound-array re-read.
`TD-OILS-A-DOUBLE-QUOTE-IN-A-BARE-SPLICE-DOES-NOT-CLOSE-THE-ENCLOSING-RUN` (the
`"` case) is now fixed too — and its fix **replaced** the segment split described
above: where the splice makes the expansion's read of the word disagree with the
parser's *at all*, the whole word is now handed back as text for the expander to
re-read. The splitting shape above — `"$(echo ${x:-$'a\tb'})"` — is a third, and
is fixed too:
`TD-OILS-A-BRACE-BODY-IN-A-QUOTED-COMMAND-SUBSTITUTION-DOES-NOT-INHERIT-THE-QUOTE`
below. bash keeps `P_DQUOTE` for a `${ }` in a `$( )` body enclosed by quotes
because its `dstack` is never popped for the `(`.
