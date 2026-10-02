### TD-OILS-COND-ERROR-LINE-AFTER-A-CONTINUATION — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/parser.rs` — the line a syntax error is blamed on,
and the source line echoed under it.

**What.** With a line continuation between the offending token and the
one-character lookahead that follows it, bash blames the *later* line:

```text
[[ P;\
Q ]]
              bash: line 2: unexpected token `;', conditional binary operator expected
                    line 2: syntax error near `Q'
                    line 2: `Q ]]'
              osh:  line 1: … (same first line, same `near `Q'`) …
                    line 1: `[[ P;\'
```

The ``near `Q'`` text agrees — that is what
TD-OILS-COND-SYNTAX-ERROR-NAMES-ONLY-THE-TOKEN fixed. Only the line *number* and
the echoed line differed.

This entry originally claimed the divergence was specific to `[[ ]]`, on the
grounds that "ordinary (non-conditional) errors already agree". **That was
wrong** — measuring them showed the same one-line shift everywhere a
continuation is written flush after the offending token, with no conditional in
sight:

```text
echo a ;;\             bash: line 2: syntax error near unexpected token `;;'
b                            line 2: `b'
                       osh:  line 1: … / line 1: `echo a ;;\'
```

`echo a )\<nl>b`, `foo() ;\<nl>x`, `case x in esac ;;\<nl>y` and
`if true; then :; fi ;;\<nl>x` all behaved the same way.

**Why.** bash reports at `line_number` — *the reader's* current line — and
echoes `shell_input_line`, which the reader has already refilled with line 2 by
the time it peeked past `;`. `shell_getc` deletes a `\<newline>` by bumping
`line_number` and fetching the next line, so a token with one written flush
after it leaves the reader a line further down than the token ends.

**Fixed** by reporting the *reader's* line rather than the token's:
`Parser::reader_line` = `cur_line()` plus `Spans::cont_lines`, the number of
continuations the reader crossed getting past the token. Both central stamping
sites (`parse_tokens` and `IncrementalParser`) now use it, so every diagnostic —
conditional or not — moves, and the echoed source line follows for free because
osh echoes whichever line it reports. No per-physical-line input buffer was
needed after all.

`cont_lines` counts a continuation on *either* side of the recorded token span,
because the lexer does not draw that boundary the same way everywhere: after
`;;` it deletes the continuation before stopping, so the span ends in one, while
after `)` it stops first and the continuation is still ahead. Both are the same
crossing. The two cannot double-count, since a span ending in a continuation
leaves the forward walk starting past it.

**Pinned by** `tests/corpus/a-continuation-after-a-token-moves-the-error-down-a-line.sh`,
which also covers the three things that do *not* move the error (a space before
the continuation, a plain newline, a tab) and the long runs that move it more
than one line. The `sed` workaround in
`a-conditional-error-quotes-the-source-not-the-token.sh` is gone; that case is
now compared whole.

**Left behind:** TD-OILS-AN-OPERATOR-CROSSES-A-CONTINUATION-ONLY-IF-IT-LOOKS-PAST-ITSELF
— the shapes where the reader stops short of the continuation, or reaches past
it, rather than landing on it. Also fixed, the same day.
