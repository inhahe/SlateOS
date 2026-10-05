### TD-OILS-AN-ARITHMETIC-COMMAND-NEEDS-ITS-CLOSERS-ADJACENT — 2026-08-05 — ✅ FIXED 2026-08-05

*(was TD-OILS-ARITH-COMMAND-CLOSES-ACROSS-A-CONTINUATION, which described only
the continuation case; measuring the rest showed the rule is much broader and
one of its consequences was a behavioural, not cosmetic, divergence.)*

**Where:** `userspace/oils/src/lexer.rs`, `Lexer::read_arith_body` and the `'('`
arm of the tokenizer.

**What.** `(( … ))` is an arithmetic command **only when the two closing
parentheses are literally adjacent**. Anything at all between them — a space, a
tab, a newline, a deleted line continuation — and bash re-reads the whole thing
as *nested subshells* `( ( … ) )`. osh instead scanned on and failed:

```text
((echo hi) )              bash: hi                     was: syntax error: malformed …
((a=1) ); echo "a=$a"     bash: a=                     was: syntax error: malformed …
((1+1) )                  bash: 1+1: command not found was: syntax error: malformed …
((1+1)  )                 same                         was: same
((1+1)<tab>)              same                         was: same
(( 1+1 ) )                same                         was: same
((1+1)<nl>)               same                         was: same
```

The first two rows were the serious ones: bash *runs* `echo hi` and *runs* an
assignment confined to a subshell, where osh raised a syntax error.

Everything adjacent already agreed, including every other place a continuation
may be split, and the whole `$(( … ))` expansion family:

```text
((1 +\<nl>1)) && echo hit   bash: hit  osh: hit   (the body)
(\<nl>( 1+1 )) && echo hit  bash: hit  osh: hit   (the opening pair)
((1+1))\<nl> && echo hit    bash: hit  osh: hit   (after the closers)
echo $((1+1)\<nl>)          bash: 2    osh: 2
echo $((1+1) )              bash: 1+1: command not found — and so does osh
echo $((1+1)<nl>)           bash: line 2: 1+1: command not found — and so does osh
```

**Why.** bash reads the body with `parse_matched_pair`, which has
`remove_quoted_newline` on — hence the body and the opening `((` tolerate
splits. `parse_arith_cmd` then tests for the second `)` with `shell_getc (0)`,
which does not remove, so only an adjacent one counts; on failing it pushes
`(` + body + `)` + *the one character it read* back into the input and returns a
plain `(`, so the ordinary grammar builds `( ( … ) )`. `$(( … ))` reaches the
substitution path, which never runs the test, which is why the expansion column
agrees throughout.

**Fixed by** giving the body scan the caller's rule and a way to fail that is
not an error. `read_arith_body(adjacent)` returns `Ok(None)` when the body
balanced but the second `)` was not where the caller requires it, reserving
`Err` for running out of input — which is an error either way, since
`parse_matched_pair` fails outright there and `parse_arith_cmd` passes the
failure on without rewinding. The `((`-command arm then rewinds `pos` to the
second `(`, truncates the continuations the abandoned scan deleted (the same
text is about to be read again, and would otherwise record them twice) and
emits a plain `(`.

A `for` header is the exception, and is *not* rewound: bash tests it for the
same adjacency but through the `ARITH_FOR_EXPRS` arm, which has nothing to fall
back to — `for` cannot be followed by a subshell — so a header that fails the
test is simply an error.

**Pinned by**
`tests/corpus/an-arithmetic-command-needs-its-closing-parentheses-adjacent.sh`,
and — for how the `for` header's failure reads —
`tests/corpus/an-arithmetic-for-header-that-fails-the-adjacency-test-is-blamed-by-position.sh`.
What is left over is message-only, and is
TD-OILS-A-REWOUND-ARITHMETIC-COMMAND-IS-NOT-REWOUND-THE-WAY-BASH-REWINDS-IT.
