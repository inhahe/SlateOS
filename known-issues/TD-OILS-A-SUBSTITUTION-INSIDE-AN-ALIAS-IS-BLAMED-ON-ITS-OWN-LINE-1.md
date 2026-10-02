### TD-OILS-A-SUBSTITUTION-INSIDE-AN-ALIAS-IS-BLAMED-ON-ITS-OWN-LINE-1 — 2026-08-05 — ✅ MOSTLY FIXED 2026-08-05

**Where:** `userspace/oils/src/lexer.rs` — `expand_aliases_inner`, which
re-tokenizes an alias value with `tokenize_spanned(val, opts)`. That lex starts
its line numbering at 1, and a `$( … )` in the value carried those numbers out
with it.

**What.** A parse error inside a command substitution written in an *alias
value* was reported at line 1 of the value rather than at the alias's call site:

```sh
shopt -s expand_aliases
alias A="echo \$( ! )"
A
```

```text
bash: line 3: syntax error near unexpected token `)'   line 3: `A'
osh:  line 1: syntax error near unexpected token `)'   line 1: `shopt -s expand_aliases'
```

The same substitution written directly was right (`echo $( ! )` on line 2
reports line 2), so this was the alias path alone.

**Fixed (the line) 2026-08-05** by `reline_tok` in `expand_aliases_inner`: a
replacement's tokens already inherit the alias word's line, and now so do the
lines a token carries as *payload* — the `)` line a `Seg::CmdSub` remembers and
the `(` line a `Seg::ProcSub` does. bash bumps `line_number` only on the fetch
of an input line (parse.y 2346), and reading a pushed alias string is not a
fetch, so a replacement genuinely has no lines of its own. Pinned by
`tests/corpus/a-substitution-in-an-alias-value-is-reported-at-the-call-site.sh`.

**Fixed (the echoed line) 2026-08-05** by `Parser::echo_at`. The echo used to
be stamped centrally from the parser's *current* token
(`Parser::reader_echo`, taken at `Parser::pos`), but a substitution's body is
parsed while a word is lowered, and every caller has stepped past that word by
then — so the lookup placed bash's reader one token too far. One token's worth
of extra reading is exactly what pops an exhausted alias replacement, so the
error was echoed against the script's own line even where bash was still
standing in the value:

```text
alias A="echo $( for ) tail";  A     bash echoes `echo $( for ) tail'
```

`Parser::word_from_segs_at` now takes the index of the word it is handed and
stamps `ParseError::echo` from *its* end, through the same
`Spans::echo_line`/`reader_stop` walk the central path uses. The two answers
then differ exactly where they should: a substitution with text after it in the
value leaves the reader inside the replacement and echoes it, while one that
ends the value is read past, popped, and echoed against the script. The index is
an explicit parameter rather than `Parser::pos - 1` because the callers are not
uniform — `parse_case`, the case-pattern loop and both `try_assignment` arms
convert *before* bumping — and getting it wrong is silent.

**Still open: a nested alias, and a multi-line alias value.**

```text
alias B="echo $( for )"; alias A="B"; A
                                     bash echoes ` '                    osh `A'
```

The reader pops out of B's replacement (the `)` ends it), then out of A's (the
`B` ends *that*), and bash is left standing in a one-character text that is
neither replacement nor the script line. The likely source is the synthetic
END_ALIAS space `shell_getc` returns at the end of a pushed alias string
(parse.y 2614–2642) rather than popping it — osh has no counterpart for that
space, which is also why it never shows up in the `near` slice. Pinning it wants
its own probe pass over what that space does to a *nested* push.

Separately, a **multi-line** alias value is still numbered wrongly inside the
substitution:

```sh
shopt -s expand_aliases
alias A="echo \$(
for
)"
A                    # bash: line 5, echoing the whole replacement
                     # osh:  line 4, echoing `)"'
```

`parse_cmdsub_body` derives the body's physical numbering from
`close_line - (newlines in the body) - 1`, which assumes the body's lines are
consecutive lines of the script. In a replacement they are all one line, so the
subtraction has to be skipped — the body's every line is `close_line`'s. That
wants `parse_cmdsub_body` to be told the body came from a text with no lines of
its own, which is the same fact `Spans` already records as a `TokSpan::src`
other than `0`.

**Impact.** Cosmetic, and confined to a substitution that fails to parse inside
an alias value. Predates the `TokSpan` work (verified against the parent
commit).

**Found by** the probe matrix for
TD-OILS-COND-ERROR-NEAR-IGNORES-THE-ALIAS-TEXT.
