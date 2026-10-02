### TD-OILS-COND-TOKEN-SPELLING. `syntax error near` names a token where bash re-scans the raw source line — 2026-07-28 — PARTIALLY RESOLVED 2026-07-28 (diagnostic wording only)

**Where:** `userspace/oils/src/parser.rs` — `parse_cond` / `parse_cond_primary`
and the shared helper `cond_error_near`.

**Fixed 2026-07-28.** Two of bash's rules were missing and are now implemented:

* Where a **conditional binary operator** was expected, bash names the offending
  token when it is an *operator* and stays silent about it when it is a *word*:
  ``[[ a ( b ]]`` → ``unexpected token `(', conditional binary operator
  expected``, while ``[[ a b ]]`` → bare ``conditional binary operator
  expected``. osh only did this for a newline; it now does it for every operator
  except `&&`, `||` and `)`, which may legitimately follow a finished operand.
* The same distinction governs the **stray-token-before-`]]`** path, where bash
  appends ``: unexpected token `X'`` for an operator and nothing for a word. osh
  hard-coded that suffix for `)` alone; it now applies to any operator.

`cond_error_near` also reproduces bash's odd *near* text for compound operators:
`;;` reports near `;`, and `;&`, `;;&`, `|&`, `>&` all report near `&`, while
`>>` and `<<<` are printed whole. Verified against bash 5.2.37 for `(`, `;`,
`|`, `&`, `>>`, `<<<`, `;;`, `;&`, `;;&`, `|&`, `>&` in both positions.

**Still open — the raw-source scan.** bash does not name a *token* in the
`syntax error near` line at all. `error_token_from_text` (parse.y) walks the
**source line** backwards from the end of the offending token, stopping at any
of `" \n\t;|&"`, and prints that span. For anything written with surrounding
whitespace the result equals the token, which is why the cases above now match;
but an operator written flush against its neighbour picks the neighbour up:

```sh
[[ -n @(a) ]]     # bash near `@(a'        osh near `('
[[ @(a) == b ]]   # bash near `@(a'        osh near `('
[[ a>>b ]]        # bash near `a>>b'       osh near `>>'
[[ x|y ]]         # bash near `|y'         osh near `|'
[[ a $(echo x) ]] # bash near `x)'         osh near `$(echo x)'
```

The closing `]]` is the same story, re-measured 2026-07-31: because the scan
stops at `" \n\t;|&"`, a `;`/`|`/`&` written flush against `]]` *becomes* the
whole reported span, while a `)` — not a stop character — is glued onto it:

```sh
[[ -n ]];echo z    # bash near `;'     osh near `]]'
[[ -n ]] ;echo z   # bash near `]]'    osh near `]]'   (a space and they agree)
[[ -n ]]&&echo z   # bash near `&'     osh near `]]'
[[ -n ]]|cat       # bash near `|'     osh near `]]'
[[ -n ]])          # bash near `]])'   osh near `]]'
```

Both shells agree the input is a syntax error and both exit 2; only the quoted
span differs, and only when tokens are written without separating whitespace.
The rest of the diagnostic — the first line (``unexpected argument `]]' to
conditional unary operator``) and the echoed source line — already matches in
every case above, which is why `tests/corpus/test-arity-errors.sh` and the
`[[ … ]]` corpus cases pass despite this.

**Proper fix:** give `Parser` the source text and the per-token character
offsets (`Tokenized::offsets`, added for TD-OILS-EXTGLOB-UNGATED) and compute
the near-text by bash's backward scan instead of from the token. The obstacle is
alias expansion: `expand_aliases` splices tokens that have no source position,
so the offsets would need an `Option` per token — the same shape
`IncrementalParser::work_origin` already uses. `tests/corpus/extglob-lexing.sh`
currently drops stderr on the two probes that hit this; restore it when fixed.
