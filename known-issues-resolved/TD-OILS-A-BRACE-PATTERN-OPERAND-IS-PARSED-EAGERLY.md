### TD-OILS-A-BRACE-PATTERN-OPERAND-IS-PARSED-EAGERLY. `${y#$(fi)}` is read as source at parse time, so it either kills the whole script or vanishes without a word — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/parser.rs`. The pattern/replacement/offset
fragments of a `${ … }` all go through `word_verbatim_from_source_at`, which
always passes `Quoting::Bare` to `seg_to_part` (parser.rs:6954). `Quoting::Bare`
means "the parser read this", so a nested `$( … )` is parsed **now**. The operand
of `${x:-w}` does not have this problem: `operand_from_source` (parser.rs:6989)
routes a non-bare read to `lex_operand_in_dquote(s, q == Quoting::Unread)` and
keeps the body as `CmdSubBody::Unread`. The call sites that want the same
treatment are parser.rs:6640, 6658, 6835, 6874, 6889 (patterns, replacement,
case-modification pattern, substring offset).

**Reproduce** (`y=Y`, bash 5.2.37):

| value, expanded with `@P` | bash | osh |
|---|---|---|
| `A${y#$(fi)}B` | 2 × `command substitution: … `fi)}B'` + `A${y#$(fi)}B: bad substitution`, result `A${y#$(fi)}B` | result `A${y#$(fi)}B`, **no diagnostics at all** |
| `A${y/$(fi)/z}B` | same shape, tail `` `fi)/z}B' `` | same silence |
| `A${y/Y/$(fi)}B` | same shape | same silence |
| `A${y^$(fi)}B` | same shape | same silence |
| `A${y:$(fi):1}B` | same shape, tail `` `fi):1}B' `` | same silence |

In a here-document body the same eagerness is worse than silent — it kills the
*whole script*:

```
$ printf 'y=Y\ncat <<E\nA${y#$(fi)}B\nE\n' > s.sh
bash: (runs; reports the substitution and prints `A${y#$(fi)}B`)
osh:  s.sh: line 1: syntax error near unexpected token `fi'
      s.sh: line 1: `y=Y'
```

**Why it matters now.** `Shell::brace_extent_scan` (added for
TD-OILS-A-FAILED-EXTENT-PARSE-INSIDE-A-BRACE-OPERAND-DOES-NOT-KILL-THE-BRACE) is
what makes these report, and it cannot see these substitutions because the parse
never produced a `WordPart::CommandSub` for them — it produced a `ParseError`
that the caller swallowed by falling back to the raw text.

**Fixed.** `crate::lexer::lex_word_verbatim_opts`, `lex_replacement_verbatim` and
`tokenize` each grew an `unread` flag, the way `lex_operand_in_dquote` already
had one, and the caller's `Quoting` is threaded through
`word_verbatim_from_source_at` / `word_replacement_from_source` /
`word_from_source` to every fragment of a `${ … }`. The fragments then produce
`CmdSubBody::Unread` bodies, and the existing brace scan and the existing
string-level extent read reach them with no further work.

**What the plan above got wrong: the two questions are independent.** "Was this
text read by a parser" and "what quoting does it expand under" are *not* the same
bit, and a pattern answers them differently from the operand beside it. Measured
in a here-document body with `v=$'a\tb'`: `${nope:-$'a\tb'}` prints `$'a\tb'`
back while `${v#$'a\tb'}` trims to nothing, and the split holds one level down —
`${v#${z:-$'a\tb'}}` trims too. Naively marking a pattern as here-doc text would
have broken all three, which osh already had right.

The mechanism is **not** `getpattern`, though that is what the first write-up of
this fix claimed. `getpattern` (subst.c:5751-5754) does replace the enclosing
`Q_HERE_DOCUMENT`/`Q_DOUBLE_QUOTES` with `Q_PATQUOTE`, and that is the *quoting*
half — but it cannot be the ANSI-C half, because `expand_word_internal`
translates no `$'…'` at all. That is exactly why bash carries a separate
`expand_string_dollar_quote` "for code paths that don't do it"
(subst.c:4171-4172), and it is confirmed by a second measurement: a runtime array
subscript is expanded with `expand_subscript_string (sub, 0)` — quoting-wise as
bare as a pattern — and `[[ -v "m[\$'a\tb']" ]]` is still *false* against a real-tab
key, so nothing translated it.

Translation is the reader's, and a here-document body had no reader. bash puts it
back for exactly one span: the fragment after a `#`, `%`, `/`, `^`, `,` or a
substring `:`, which `parameter_brace_expand` re-extracts with `SX_POSIXEXP`
(subst.c:9913), and which — inside a here-document — is routed (subst.c:1828-1832)
to `extract_heredoc_dolbrace_string`, a function whose own comment says it exists
"to handle `$'...'` and `$"..."` quoting in here-documents, since the
here-document read path doesn't" (subst.c:1522-1530). `:-`, `:+`, `:=` and `:?`
are not on that list — the `:` is eaten as the null-check before the operator is
read — so an operand keeps its text. And the one-level-down case falls out of the
same function: it scans the whole fragment with `dolbrace_state` pinned at
`DOLBRACE_QUOTE` (every transition it has leaves `DOLBRACE_PARAM`, which it never
reaches), so a nested operand is translated along with the pattern around it.

osh's flag still names the *quoting* rather than the operator, because clearing
one flag gives both answers: the fragment is extracted with the translation and
expanded outside the double-quoting, and no fragment gets one without the other.

So `Lexer::here_text` was split into the two flags it had been standing in for —
`here_text` (the reader's half: no parser read this, so a `$'…'` is untranslated
and a `$( … )` has no parse) and the new `Lexer::dq_context` (the quoting half:
this text expands in `Q_DOUBLE_QUOTES`/`Q_HERE_DOCUMENT` as a whole) — and
`parser::Quoting` grew its fourth state, `Quoting::BareUnread`, with
`Quoting::as_pattern` naming the transition every fragment call site takes.

**Two fragments the write-up above missed**, both fixed with the rest:

- **The substring bounds** go through `word_from_source`, which reaches the
  general `lexer::tokenize` rather than the verbatim reader — so `tokenize` took
  the flag too. bash expands the bounds with `expand_arith_string (substr,
  Q_DOUBLE_QUOTES|Q_ARITH)` (subst.c:8134), which is a *runtime* read of a
  string like the pattern's.
- **The `[ … ]` subscript**, via `split_name_subscript`. It now holds a
  `CmdSubBody::Unread` and so reports at all, which is half of
  TD-OILS-A-BRACE-SUBSCRIPT-IS-NOT-EXPANDED-AS-A-STRING-OF-ITS-OWN — see that
  entry for what is still wrong (the tail is the whole word's, not the
  subscript's).

**Corpus:**
`a-pattern-of-an-unread-brace-is-read-with-the-brace-s-own-read.sh`.

**Found by** the corpus pass for
`a-failed-extent-parse-inside-a-brace-operand-does-not-kill-the-brace.sh`.
