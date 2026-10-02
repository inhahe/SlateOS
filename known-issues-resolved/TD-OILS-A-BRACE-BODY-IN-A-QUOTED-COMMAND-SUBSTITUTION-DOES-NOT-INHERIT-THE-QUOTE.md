### TD-OILS-A-BRACE-BODY-IN-A-QUOTED-COMMAND-SUBSTITUTION-DOES-NOT-INHERIT-THE-QUOTE. `"$(echo ${x:-$'a\tb'})"` splits in bash — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/lexer.rs` — the `dq` argument threaded through
`read_subst_body` → `read_balanced_inner` → `read_balanced_body` →
`read_opaque_span`, which is what the `${` arm hands `read_dollar_brace`.

**What.** Which of the three re-quote rows a `$'…'` in a `${ … }` body takes
turns on `P_DQUOTE`, and `P_DQUOTE` is **not** "the `${` was written inside
quotes". It is

```c
rflags = (qc == '"') ? P_DQUOTE : (flags & P_DQUOTE);     /* parse.y:3696 */
```

where `qc` is the `cd` that `read_token_word` hands down —
`parse_matched_pair (cd, '{', '}', …)` (parse.y:5033) — and `cd` is
`current_delimiter (dstack)`. Only three sites push onto that stack, all of them
in `read_token_word`:

| site | pushes | reached by |
|---|---|---|
| parse.y:4952 | the quote character | a `'`, `"` or `` ` `` starting a word |
| parse.y:5041 | `(` | a `$(` starting or continuing a word |
| parse.y:5071 | `(` | a `<(` / `>(` |

`parse_matched_pair`'s own nested-`$(` arm pushes **nothing**:

```c
nestret = parse_comsub (0, '(', ')', &nestlen, (rflags|P_COMMAND) & ~P_DQUOTE);
                                                            /* parse.y:3960 */
```

and clearing `P_DQUOTE` in those flags buys nothing, because `parse_comsub`
(parse.y:4083) runs a whole nested `yyparse` (parse.y:4153) — the flag never
reaches the body's own `read_token_word`. What reaches *that* is the `dstack`,
which still has the enclosing `"` on top, because `read_token_word` pushed it at
4952 and does not pop until the whole `"…"` is read.

So a `$( … )` **written inside double quotes** hands its body the `"`, and every
`${ … }` the body reads directly is `P_DQUOTE` — spliced bare — even though
nothing in the body is quoted. Measured (`x` unset; printbacks are `declare -f`,
i.e. the token text after the parser's own translation):

```text
                                        bash                    osh (before)
"$(echo ${x:-$'a\tb'})"       printback  ${x:-a<TAB>b}           ${x:-'a<TAB>b'}
                              runs to    [a b]                   [a<TAB>b]
$(echo ${x:-$'a\tb'})         printback  ${x:-'a<TAB>b'}         same
"$(echo $(echo ${x:-…}))"     printback  ${x:-'a<TAB>b'}         same
"$(echo "$(echo ${x:-…})")"   printback  ${x:-a<TAB>b}           ${x:-'a<TAB>b'}
"$( { echo ${x:-…}; } )"      printback  ${x:-a<TAB>b}           ${x:-'a<TAB>b'}
"$(cat <(echo ${x:-…}))"      printback  ${x:-'a<TAB>b'}         same
"`echo ${x:-$'a\tb'}`"        printback  ${x:-$'a\tb'}           same
```

The rows that already matched are exactly the ones that push something of their
own: a nested word-level `$(`, `<(` or `>(` puts a `(` on top; a `"…"` run in the
body puts its own `"` there (which is `P_DQUOTE` again anyway). A `` ` … ` `` is
matched *as source* (parse.y:3953, no parse), so the `$'…'` in it is never
translated at parse time and the expansion's own read starts from an empty
stack. A `{ … }` or `( … )` **group** in the body pushes nothing — so it does
not clear the quote, which the fifth row is the proof of.

The difference is live, not cosmetic: the bare splice's tab is a real tab in the
command the `$( … )` re-parses, so it **splits** (and a `$'a*b'` there would
glob). A spliced `}` closes the body's own expansion, and a spliced NUL cuts the
substitution's body where it lands (`nestlen = ttranslen`, parse.y:3892), so the
`$( … )` keeps a body that stops mid-expansion and fails to re-parse.

**Fixed by** threading a `dq` flag — bash's `current_delimiter (dstack)`,
narrowed to "is the delimiter this scan stands in a `"`?" — through the balanced
scan, instead of the previous "am I inside a double-quoted run?" test. The
`${` arm of `read_opaque_span` passes it to `read_dollar_brace`, and the loop
that reaches it passes `dq && nested.is_empty()`, where `nested` is the running
stack of nested `$(`/`<(`/`>(` — i.e. exactly bash's `push_delimiter (dstack,
'(')`. Call sites that start a fresh read with an empty stack
(`scan_cmdsub_body`'s re-read, a word-level procsub, an arithmetic scan) pass
`false`; `read_dollar`'s `$(` arm passes the enclosing `in_dquote`.

**The recorded note was wrong, and the measurement corrected it.** The prior
text (and a comment in `read_dollar_brace_body`) said a nested `$(` *strips*
`P_DQUOTE`, reading parse.y:3960 literally. It does strip it from the flags —
and the flags are not what decides.

Corpus case:
`userspace/oils/tests/corpus/a-brace-body-in-a-quoted-command-substitution-stands-in-the-quote.sh`.
