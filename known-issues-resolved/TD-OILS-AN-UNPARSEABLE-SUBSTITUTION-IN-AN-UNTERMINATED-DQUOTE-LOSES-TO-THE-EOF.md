### TD-OILS-AN-UNPARSEABLE-SUBSTITUTION-IN-AN-UNTERMINATED-DQUOTE-LOSES-TO-THE-EOF. bash blames the `$( … )`, osh blames the quote — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/lexer.rs`, `Lexer::read_double_quote_until` — it
scans a `" … "` for its closing quote and raises the unterminated-quote error at
end of input, having lexed the `$( … )` inside it as an *unread* body that no
one ever parses. bash's `parse_matched_pair` instead calls `parse_comsub` the
moment it meets `$(` (parse.y), so the substitution's own syntax error is raised
first and the missing `"` is never reached.

**Reproduce.**

```sh
( eval 'echo " $(fi)' ) 2>&1; echo "rc=$?"
```

| | bash 5.2.37 | osh |
|---|---|---|
| stderr | ``syntax error near unexpected token `fi'`` + ``` `echo " $(fi)' ``` | ``unexpected EOF while looking for matching `"'`` |
| `$?` | 1 | 2 |

The same with a `'` in the way (`echo " ' $(fi)`) — the `'` is inside the quotes,
so it is an ordinary character to both, and the disagreement is unchanged.

A backquote is *not* affected: `` echo `x $(fi) `` gives both shells
``unexpected EOF while looking for matching ``'`` and `$?` 2, because a
backquote body is read as text to its mate and nothing inside it is parsed on
the way.

**Why the difference matters beyond the message.** The exit status differs too —
2 (a lexer-level unterminated construct) against 1 (a parse error) — so a script
branching on `$?` from an `eval` of untrusted text sees a different value.

**The fix.** Parse a `$( … )` met inside a double-quoted body as bash does,
where its error is raised as it is met rather than deferred to an unread body.
The care needed is that `$( … )` bodies are deliberately left *unread* in many
places (`CmdSubBody::Unread` exists because a body that will not parse must not
be a parse error when nobody reads it — see
TD-OILS-A-QUOTE-RUN-IN-A-PATTERN-OPERAND-IS-NOT-GOBBLED); this is the one case
where the enclosing construct never closes, so there is no word to defer to and
bash's own reader has already committed to the substitution.

**Fixed 2026-08-14, along exactly the [`SubstBail`] line one construct further
out.** The bodies are still lexed where they were, and are carried *out on the
error* to be parsed by the one place that has `ParseOpts` in hand.

**The scope is the whole word, not the quote.** The quote is not what makes the
body eager — the *word* is, because a word that never finished yields no token
at all, so its bodies would otherwise be parsed by nobody. Measured:
`echo $(fi)x$(`, `echo $(fi)x${y`, `echo $(fi)x$((1+`, `echo $(fi)x"y`,
`echo $(fi)x'y` and ``echo $(fi)x`y`` all name `fi`, with no quote in sight. An
*earlier* word needs none of this — it finished, so it is a token the parser
reaches on its own (`echo a$(fi) b$(`, `echo a$(fi); echo b$(`).

- `LexError::eager_bodies: Option<Box<Vec<EagerBody>>>` — the substitution
  bodies the word already read *and parsed where it met them*, in reading order.
  `EagerBody { src, line, procsub }`: `procsub` picks the reference line, since
  a `$( … )` body is numbered from its `)` (`parse_cmdsub_body`) while a
  `<( … )` / `>( … )` body is numbered from its *opening* delimiter
  (`parse_procsub_body`), a procsub being a child command. Both land on the line
  the offending token is physically written on.
- Filled by `lexer::eager_bodies_in`, which walks the finished segments for
  `Seg::CmdSub(_, _, SubBody::Eager)`, for `Seg::ProcSub` (measured:
  `echo <(fi)x$(` and `echo >(fi)x$(` both name `fi`), and for the spans a
  `Seg::ParamBraced`/`Seg::Arith` stepped over (`echo " ${x:-$(fi)}`,
  `echo " $(( $(fi) ))`), recursing into a closed `Seg::Dq` (`echo "a$(fi)"x$(`).
  A `SubBody::Backtick` is deliberately *not* collected: that body is read as
  text now and parsed only at expansion time, which is exactly why the backquote
  rows disagree with nothing (``echo " `fi` ``, ``echo `fi`x$(``).
- Attached by the `read_word_inner` wrapper, which was split so that
  `read_word_segs` writes into the caller's `segs` — a word that gives up part
  way therefore still leaves behind what it read. `read_double_quote_until`
  attaches at all three of its failing exits too, not just the end-of-input one,
  because the scan can run out *inside* a later substitution and the bodies read
  before that one still win: `echo " $(fi) $(done` names `fi`.
- `parser::eager_body_error`, consulted by `resolve_subst_bail` *before* the
  `bail` path for the same reason, returns the first body error that is not
  `is_incomplete()` — a body that merely ran out said nothing (bash's
  `EOF_Reached` path), which is where `echo " $(a |` keeps its
  `` matching `)' ``.

Verified by
`userspace/oils/tests/corpus/a-substitution-inside-an-unfinished-word-is-parsed-before-the-word-gives-up.sh`
(27 rows, all matching bash 5.2.37 in message *and* rc) and the unit test
`parser::tests::a_body_read_inside_a_word_that_ran_out_is_parsed_anyway`. That
test cannot cover the *earlier-word* rows: its `parse` helper goes through
`tokenize_spanned`, which fails hard, where the shell uses `tokenize_deferred`,
which keeps a failing line's completed tokens. Those rows are corpus-only.

The same change closed the last two shapes of
TD-OILS-A-COMSUB-THAT-NEVER-CLOSES-HIDES-THE-ERROR-INSIDE-IT below.

**Found by** the flat-state gobbler fix below: the repro there
(``echo "p`echo " ' $(fi)`q{,}"``) agrees on the *scan* now, and what is left of
its divergence is entirely this.
