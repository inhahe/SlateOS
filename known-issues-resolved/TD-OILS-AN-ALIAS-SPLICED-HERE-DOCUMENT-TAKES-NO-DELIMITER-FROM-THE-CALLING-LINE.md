### TD-OILS-AN-ALIAS-SPLICED-HERE-DOCUMENT-TAKES-NO-DELIMITER-FROM-THE-CALLING-LINE. `alias B='cat <<'` plus `B E` runs `cat E` — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::lex_heredoc_op` /
`read_heredoc_delim`, which read the delimiter word out of the text the `<<` was
lexed in, and `expand_aliases_inner`, which lexes an alias replacement as a text
of its own.

**What.** A here-document operator whose *delimiter* is not in the alias value:

```
shopt -s expand_aliases
alias B='cat <<'
B E2
body
E2

bash: body
osh : cat: E2: No such file or directory        — the operator dropped,
      line 4: body: command not found             the delimiter an argument
      line 5: E2: command not found
```

**Why.** bash reads the delimiter with `read_token_word`, from the same reader
that read the `<<`; when the pushed string runs dry `pop_string` restores the
calling line and the scan simply continues into it, so `<<` + `E2` is one
operator-and-delimiter pair spanning the text boundary. osh sub-lexes the value
on its own, and `read_heredoc_delim` finds nothing after the `<<`.

Since TD-OILS-A-HERE-DOCUMENT-OPERATOR-WITH-NO-DELIMITER-WORD-WAS-NOT-A-GRAMMAR-ERROR
was fixed, such a value's tokens simply end in a bare `Tok::Op(Op::DLess)` with
no `HereDoc` after it, so the condition is plain to see in the stream (a `<<`
that *did* get a delimiter is always followed by one) and nothing marks it
specially. The bare operator then reaches the parser as a redirection with no
target, which is bounded but wrong: bash has a here-document here.

The shapes to match, all measured:

```text
alias B='cat <<'   B E2        delimiter `E2`
                   B "E2"      quoted — the body does not expand
                   B E$x       literal delimiter `E$x`; no expansion of it
                   B E4; echo  `E4`, and the `; echo` still runs
                   B W         `W` — *not* alias-expanded (not a blank-ended value)
alias S='cat << '  S W         `E9` — W *is* expanded, S's value ends in a blank
                   B E\<nl>X   `EX` — the continuation is deleted first
alias D='cat <<-'  D E3        `<<-` strips tabs as usual
alias P='cat <<E'  P 8         `E`, and `8` is an argument — the calling line's
                                leading blank ends the word at the text boundary
```

**As fixed.** The delimiter is read from the *source text* that follows the alias
word, never from a token — `B E$x` wants the literal delimiter `E$x`, and the
outer word token has already split that into a `Seg::Lit` and a `Seg::Param`.

* `AliasInput` carries `text: &[Ch]`, the characters its `lines`/`ends` index
  into, so `expand_aliases_inner` can look at the text it is walking, not just at
  its tokens. `expand_aliases_tracked` takes it as a parameter (both call sites in
  `parser.rs` already had the `Vec<Ch>` to hand).
* `expand_aliases_inner` returns a `bool`: "this text ended *at* a `<<`", which is
  true exactly when it emitted something and its last token is a bare
  `Tok::Op(DLess | DLessDash)`. That is bash's "the pushed string ran dry with the
  reader mid-redirection".
* On a `true` from the recursive splice, `take_dangling_delim` runs
  `read_delim_at` over *this* text from `ends[i]` — the character after the alias
  word, which is precisely where `pop_string` (parse.y:2694) puts the reader back
  — and, on a word, appends the `HereDoc` token, records it in
  `AliasExpansion::heredocs`, and returns the offset it stopped at. The caller
  then skips every outer token whose characters the delimiter consumed, so the
  `work` stream never contains them twice.
* `read_delim_at` distinguishes three outcomes, and the distinction is the whole
  point: `Word` (a delimiter), `Exhausted` (this text is spent too, so the `bool`
  propagates to the caller one text further out), and `None` — a separator or a
  comment stands there. Only `Exhausted` continues the chain. A `None` is an
  *answer*, obtained inside this text, so the reader stops asking: `Dangle::Sealed`
  suppresses the propagation, and `alias P='B ; B'` with `P E` is the grammar
  error at the `;` in the value, never a here-document reading `E` off the calling
  line. The `Exhausted` recursion *is* the `pop_string` chain, so `alias A='B'`
  over `alias B='cat <<'` takes `E` from the line that called `A`, and
  `alias V='B E'` takes it from `V`'s own value.
* `parse_redirect` now takes a plain `Tok::Word` after `<<`/`<<-` as *no target*
  rather than as a delimiter. A `<<` whose delimiter was read is always followed
  by the `HereDoc` token carrying it — that is the lexer's contract — so a bare
  word there is the token standing where the WORD should be. This only becomes
  reachable with the seam: `alias C='B #c'` seals in the value while a word still
  stands on the calling line.
* A value ending in a blank still sets `force` for the next word (bash's
  `PST_ALEXPNEXT`) — but not when the delimiter just consumed that word, hence
  `force = !took_delim && …`.

Because `read_delim_at` shares `read_heredoc_delim` with the ordinary path, every
delimiter spelling comes out the same across the seam: quoting (`B "E"`, `B \E`)
suppresses body expansion, `B E$x` is the literal `E$x`, line continuations
inside the delimiter are already deleted, and `<<-` written in the value strips
the calling line's body.

**Pinned by** the corpus case
`an-alias-spliced-here-document-takes-its-delimiter-from-the-calling-line.sh`
(without the fix its very first probe gives `status: bash=0 osh=2`) and the unit
test `an_alias_spliced_here_document_takes_its_delimiter_from_the_calling_line`.

**Left behind — and since superseded.** This fixed one member of a family with a
hand-written rule at the token level; the rest of the family (operators, comments,
unclosed quotes, the blank-ended-value re-expansion) needed the model bash
actually uses. That model — the textual splice — landed the same day under
TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE, and it
took `take_dangling_delim`, `read_delim_at`, `DelimAt`, `Dangle`,
`expand_aliases_inner` and `tokenize_alias_body` back out with it. Every shape in
the table above still holds — it is now the splice, not a rule, that produces
them. What remains from the family is
TD-OILS-AN-UNCLOSED-QUOTE-IN-AN-ALIAS-VALUE-IS-A-LEX-ERROR;
TD-OILS-A-BLANK-ENDED-ALIAS-VALUE-DOES-NOT-RE-EXPAND-A-HERE-DOCUMENT-DELIMITER
was fixed on top of the splice the next day.
