### TD-OILS-COND-ERRTEXT. `osh` `[[ … ]]` syntax-error *messages* don't match bash's multi-line "conditional expression" diagnostics — ✅ RESOLVED 2026-07-20

**Status:** RESOLVED. `parse_cond` and `parse_cond_primary` now emit bash's
context-specific two-line diagnostics, verified byte-for-byte against bash 5.2
across 15 malformed forms:
- a bare word primary followed by another word/word-operator
  (`[[ a b ]]`, `[[ a -a b ]]`, `[[ a -z ]]`, `[[ a && b c ]]`, `[[ ! a b ]]`) →
  `conditional binary operator expected` + `syntax error near \`TOK'`;
- a completed operand followed by a stray token where `]]` was expected
  (`[[ 3 -gt 2 -gt 1 ]]`, `[[ -z x y ]]`, `[[ ( a ) b ]]`, `[[ a -gt b -a c ]]`) →
  `syntax error in conditional expression` + `syntax error near \`TOK'`;
- a stray `)` (`[[ a ) ]]`) → the `: unexpected token \`)'` suffix form;
- the pre-existing operand-slot forms (`[[ -z ]]`, `[[ ( a ]]`) still match.
`wrap_parse_message` was extended to pass `conditional …` messages through
without the `syntax error: ` tag. Regression-guarded by
`cond_syntax_error_messages_match_bash`.

*Known residual (pervasive, not cond-specific):* the `syntax error near \`TOK'`
line reconstructs the token from osh's lexed segments, which do not preserve
original source quoting — so an empty/quoted operand (`[[ a "" ]]`) shows an
empty near-token where bash shows `""`. This is the same source-token
reconstruction limitation tracked elsewhere and affects all near-token
diagnostics equally.

*More of the same, measured 2026-07-28:* bash's near-text is a span of the raw
input line rather than a token at all, so it routinely carries a character from
whatever sits next to the offending token — and the neighbour it picks up is not
even consistently the one before or the one after:

```
[[ ab =~ a) ]]              bash: near `a)'    osh: near `)'
[[ ab =~ )a ]]              bash: near `)a'    osh: near `)'
[[ ab =~ (a;b);echo hi ]]   bash: near `;e'    osh: near `;'
[[ "a b" =~ (a) (b) ]]      bash: near `(b'    osh: near `('
```

Reproducing this needs the parser to keep a source span per token, which is the
same prerequisite as the quoting residual above; `cond_error_near` currently
approximates it for the trailing-operator cases only. First lines and exit
statuses match throughout, so `tests/corpus/cond-regex-word.sh` drops stderr on
the shapes it rejects.

**Original report (for reference):**

**Where:** `userspace/oils/src/parser.rs` `parse_cond` and its helpers (the
`[[ … ]]` conditional-expression parser). Every `Err(ParseError(...))` in that
path uses osh's single-line house style (`syntax error: expected ']]' to close
'[['`, `syntax error: unexpected ']]' (expected operand)`, `syntax error:
expected ')' in '[[ … ]]'`).

**What:** on a malformed `[[ … ]]` expression bash prints a **two-line**,
token-naming diagnostic that osh does not reproduce (exit codes match — both
non-zero — only the human-readable text differs). Measured against bash 5.2:

```
[[ 3 -gt 2 -gt 1 ]]   bash: syntax error in conditional expression
                            syntax error near `-gt'
                      osh : syntax error: expected ']]' to close '[['
[[ a b ]]             bash: conditional binary operator expected
                            syntax error near `b'
                      osh : syntax error: expected ']]' to close '[['
[[ -z ]]              bash: unexpected argument `]]' to conditional unary operator
                            syntax error near `]]'
                      osh : syntax error: unexpected ']]' (expected operand)
[[ ( a ]]             bash: unexpected token `]]', expected `)'
                            syntax error near `]]'
                      osh : syntax error: expected ')' in '[[ … ]]'
```

**Why deferred:** this is the same class of cosmetic stderr-text divergence as
the (resolved) arithmetic one and the `[[ ]]` errors only fire on invalid
expressions that essentially no real script contains. bash's format needs
per-context taxonomy ("conditional binary operator expected" vs "…unary
operator" vs "conditional expression") plus the offending-token name and bash's
second `syntax error near \`TOKEN'` line — osh deliberately uses single-line
diagnostics everywhere (functions, coproc, arithmetic operand errors), so
matching bash here would be an inconsistent one-off unless the whole shell moves
to bash's two-line format. Behavioral (result-affecting) divergences are higher
value.

**Proper fix (if ever needed):** classify `parse_cond` failures into an enum
carrying (a) bash's context-specific first line and (b) the offending token for
the `syntax error near \`TOKEN'` second line; emit both lines with the `osh:
line N:` prefix on each, mirroring how bash repeats the `bash: -c: line N:`
prefix per line.
