### TD-OILS-AN-UNCLOSED-QUOTE-IN-AN-ALIAS-VALUE-IS-A-LEX-ERROR. `alias U="echo 'x"` plus `U y'` never reaches the alias pass — 2026-08-06 — OPEN

**Where:** `userspace/oils/src/lexer.rs` — `tokenize_deferred`, which lexes the
whole script before any alias exists; the alias pass
(`expand_aliases_tracked`) runs on its output.

**What.** A quote or a `$(` opened in an alias value and closed on the calling
line. Measured:

```text
shopt -s expand_aliases
alias U="echo 'x"
U y'
echo next

bash: x y        — one single-quoted word, spanning the seam
      next
osh : line 3: unexpected EOF while looking for matching `''     (status 2)
```

`alias D='echo "x'` with `D y"` is the same shape with double quotes.

**Why.** The textual splice
(TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE) makes
value and calling line one text, so once the pass runs, the quote closes exactly
as bash's does — a `$(` opened in a value and closed on the calling line already
works, and is probed in the corpus. But the pass only ever sees *tokens*, and
those come from a lex of the script that happened before the `alias` builtin ran.
At that point the value is still just the inside of a string literal on the
`alias` line, and the calling line's `y'` is a lone unmatched quote. osh lexes
eagerly and reports it; bash, reading a line at a time, never has an unmatched
quote to report because by the time it reads that line the value is in front of
it.

The `$(` case survives only because the *value's* half is what is unclosed there,
and the value is not lexed on its own — the calling line's `)` closes it during
the splice's re-lex. The quote case is the reverse: the calling line's own half
is unbalanced in the pre-pass lex.

**Proper fix.** Lex lazily, a physical line at a time, with the alias pass and the
parse interleaved — which is bash's structure. (This is *more* than
TD-OILS-A-LINE-THAT-FAILS-TO-LEX-IS-NOT-REPORTED-AS-A-LINE turned out to need:
that one was fixed by keeping the failing line's tokens and letting the parser
run into them, but here the tokens are wrong to begin with, because the alias
value was never in the text the lex read.) Anything short
of that (e.g. deferring an unmatched-quote error until the alias pass has had a
look) would be a second reading of the same text under a different rule, which is
exactly the band-aid the splice replaced.

**Pinned by** nothing — the corpus case
`nothing-lexical-stops-at-the-alias-seam.sh` carries a "Not probed" note pointing
here, because probing it would make the whole script fail to lex.
