### TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE. Nothing lexical may cross the alias seam — `alias Q='cat <'` plus `Q< E` is a syntax error — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `expand_aliases_inner` and
`tokenize_alias_body`, which lexed an alias replacement as a text of its own.

**What.** Every one of these is one construct in bash, written half in the value
and half on the calling line, and a syntax error in osh. All measured:

```text
shopt -s expand_aliases
alias Q='cat <'        Q< E          `<<` — a here-document           bash: spanned
alias G='echo hi >'    G> /dev/null  `>>`                             bash: (appends)
alias P='true |'       P| echo or    `||`                             bash: (no output)
alias N='true &'       N& echo and   `&&`                             bash: and
alias S='echo a ;'     S; echo b     `;;` — and bash errors on *that*
alias U="echo 'x"      U y'          one single-quoted word, `x y`    bash: x y
alias D='echo "x'      D y"          one double-quoted word, `x y`    bash: x y
alias C='echo $(echo'  C hi)         one command substitution         bash: hi
```

osh reports `syntax error near unexpected token` for the first five, and
`unexpected EOF while looking for matching` for the quotes — it lexes the value
on its own, so the value's half of each construct is unterminated and the calling
line's half is orphaned.

**Why.** bash's `push_string` makes the replacement the current input line and
`pop_string` restores the calling line *at the character after the alias word*,
so one reader reads both: `take_operator` extends `<` into `<<` across the seam,
and a quote or a `$(` opened in the value is simply still open when the reader
arrives on the calling line. It is not that a few operators are special — it is
that there is no seam at all, at any level below the token.

**Which text a diagnostic quotes** follows from the same model, and is the one
thing a naive concatenation would get wrong. bash echoes `shell_input_line` as it
stands when the offending token has been read, i.e. **the text that token ended
in**: `alias A='B ; B'` with `A E` errors at the `;` and echoes `B ; B`, the
value; but `alias S='echo a ;'` with `S; echo b` errors at the `;;`, which
*ended* on the calling line, and echoes `S; echo b`.

One thing does *not* span it: an ordinary word begun on the calling line cannot
reach back, because the alias word is always followed by a blank or a
metacharacter — that is what ended it. `alias V='echo ab'` with `V cd` is `ab cd`,
two words, in both shells. Everything that spans does so from the value outward.

**Proper fix.** Splice *textually* rather than at the token level: on an alias
hit, lex `value ++ rest-of-this-text-after-the-alias-word` as one text, exactly as
bash reads it, and carry on scanning that. There is then no seam to special-case,
which is why this one fix covers the whole table above, the comment entry below,
the blank-ended-value re-expansion, and the here-document delimiter case that is
currently handled by hand in `take_dangling_delim`.

Two things the concatenation has to carry, and both are already available:

* **Provenance, for diagnostics.** Offsets below `value.len()` belong to the
  value's text; the rest map to `ends[i] + (off - value.len())` in the text being
  walked, which is itself possibly a concatenation — so a small segment table
  per text, composed on each splice. A token's `TokSpan` then comes from the
  segment its *end* falls in, which is exactly the "text the token ended in" rule
  measured above, so the `B ; B` / `S; echo b` echoes both fall out for free.
* **`AliasExpansion::origin`, which `IncrementalParser` needs to resume.** A
  token gets `Some(k)` only when it lies wholly in the input text — its start
  *and* its end map to `src == 0` — and `ends[k]` equals its mapped end. The
  extra start test is what keeps a seam-spanning token (`;;` above, whose end
  coincides with an input token's) from claiming an origin it has no right to.
  `Lexer::run_into` already records token starts in its `offsets` argument;
  `Lexer::run` currently throws them away into a scratch `Vec`, so exposing them
  on `Spanned` is all that is needed.

`take_dangling_delim`, `read_delim_at`, `DelimAt` and `Dangle` all come out again
when this lands — they are the token-level model's answer to one member of the
family.

**As fixed.** The textual splice, exactly as sketched above. `expand_aliases_tracked`
now keeps one *assembled text* — the script with every value bash pushed written in
where its alias word stood — and re-lexes the whole thing from the top of the
current physical line on every splice, so a construct written half in a value and
half on the calling line is lexed as the one construct it is. There is no seam
left to special-case, and `expand_aliases_inner`, `tokenize_alias_body`,
`take_dangling_delim`, `read_delim_at`, `DelimAt` and `Dangle` are all gone.

* `TextMap` records which run of the assembled text belongs to which source, so
  every token's `TokSpan` names the text it *ended* in — which is bash's rule for
  which line a diagnostic echoes, and gives the `B ; B` / `S; echo b` split for
  free. `AliasExpansion::origin` is `Some(k)` only for a token whose start *and*
  end map back to the input, which keeps a seam-spanning `;;` from claiming an
  origin it has no right to.
* `Lexer::spliced` starts the re-lex at the top of the caller's physical line
  (`head`), which is as far back as it must go and as far back as it may safely
  go: everything before is already run, and may no longer even be lexable (a
  here-document body it consumed is arbitrary text).
* **Here-document bodies stay in the real input.** bash reads a body with
  `read_a_line`/`yy_getc`, which bypasses `shell_input_line` and every pushed
  string, so the body of a `<<` a value brought with it is the line *after* the
  calling line. `Lexer::raw` is a second cursor for exactly that: `0` means "has
  not diverged", a gather sets it, and `run_into` skips the token cursor over
  input the body reader already ate.
* **The parser unreads what the splice swallowed.** osh lexes the whole script
  before any alias exists, so those body lines were first read as *commands*.
  `Spanned::taken` reports every span the splice lex ate as a body;
  `IncrementalParser::rebuild` loops — expand, find the first `taken` span that
  still holds unconsumed `orig` tokens (or that the first lex died inside),
  stretch the owning unit's `orig_ends` over it, `relex_tail_from` past it — with
  a `settled` watermark so a genuine lexer error in the tail cannot re-trigger the
  same span forever.
* **Line numbers follow bash's monotone `line_number`.** It is bumped by *fetching*
  a line (parse.y:2346) and never wound back, so a gather made while the reader
  stood inside a value leaves even the rest of the *calling* line numbered from
  where the gather stopped. `AliasScan::line` takes the max of the token's own
  line and the reader's, and `line_at_input` counts physical lines from the
  nearest anchoring token rather than from a token table the unread may have
  truncated.
* **Reader warnings from the splice are speculative** past the current unit — the
  pass gathers over the whole remaining text with the alias table it has *now*,
  and a later `alias` redefinition changes what it would have gathered. So
  `IncrementalParser::alias_warnings` is **replaced** by every rebuild rather than
  added to, and each warning is keyed by the offset its body *began* at
  (`Spanned::warn_from`), not by a token index into an `orig` the unread
  truncates. Keying on the body's end would not do: `rebuild` stretches the owning
  unit's `orig_ends` to that end, so an end-keyed warning falls inside every
  earlier unit too and goes out far too early.

**Pinned by** the corpus case `nothing-lexical-stops-at-the-alias-seam.sh`, which
probes every row of the table above that osh can reach, and by the two
here-document seam cases (`…-takes-its-body-from-the-real-input.sh`,
`…-takes-its-delimiter-from-the-calling-line.sh`), whose stderr is where the
warning keying shows up.

**Left behind:** TD-OILS-AN-UNCLOSED-QUOTE-IN-AN-ALIAS-VALUE-IS-A-LEX-ERROR (rows
`U`/`D` of the table — a lex-order limitation, not a seam one). The other one it
left, TD-OILS-A-BLANK-ENDED-ALIAS-VALUE-DOES-NOT-RE-EXPAND-A-HERE-DOCUMENT-DELIMITER,
was fixed the next day by giving the here-document delimiter a span of its own
and letting the scan splice over it — a small change *because* of this model.
