### TD-OILS-CMDSUB-HEREDOC-NESTED-GATHER-ORDER. two nested substitutions, each with a here-document past its close, warn in the wrong order and name the wrong lines — 2026-07-30 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `read_balanced_inner` / `gather_ahead`.
The extent scan of a `$( … )` copies its text and gathers the here-documents
declared *directly* in it, leaving a nested substitution's to be gathered when
that body is re-lexed (see TD-OILS-CMDSUB-HEREDOC-PAST-CLOSE). bash's reader is
line-based and shared across the nesting, so it does the nested gather *first* —
during the outer scan, at the nested `)` — and the outer scan's own warning is
therefore blamed on a line the reader has already advanced to.

**Symptom.** Diagnostics only; the bodies, the output and the exit status all
match. It takes a here-document past the close at *two* nesting levels on one
line:

```
$ cat y1.sh
x=$(cat <<A; echo $(cat <<B) mid); echo "x=[$x]"
aaa
A
bbb
B
$ bash y1.sh
y1.sh: line 1: warning: command substitution: 1 unterminated here-document   # the nested one
y1.sh: line 5: warning: command substitution: 1 unterminated here-document   # the outer one
y1.sh: line 5: warning: here-document at line 5 delimited by end-of-file (wanted `A')
x=[aaa A bbb mid]
$ osh y1.sh
y1.sh: line 1: warning: command substitution: 1 unterminated here-document   # the outer one
y1.sh: line 1: warning: command substitution: 1 unterminated here-document   # the nested one
y1.sh: line 2: warning: here-document at line 2 delimited by end-of-file (wanted `A')
x=[aaa A bbb mid]
```

**Cause.** Our scan warns for the outer substitution at its `)` (reader still on
line 1) and defers the nested one to the re-lex, so the two warnings come out in
the opposite order and the outer one — and the end-of-file warning that follows
from it — carry the un-advanced line.

**Proper fix.** Model bash's reader position through the nesting instead of
deferring: when the outer scan meets a *nested* `)` with here-documents pending
at that level, advance the reader over their bodies there and then, so the
reader's line is right for every later warning, and raise the nested warning from
the outer scan. The re-lex of the nested body must then be silent, which needs a
way to say "these were already reported" — the natural one is to suppress
`ReaderWarning::SubstHeredoc` in a lexer created by `Lexer::paren_body`, since
that is exactly the re-lex case. The awkward part is the raw text: the nested
body would have to be spliced at the end of the *outer* body rather than at the
nested `)`, so the ordering of the two gathers and the ordering of the two body
texts come apart and `consume_subst_heredoc_bodies` would need to take them
separately.

**Impact.** Very narrow — one line has to carry two levels of substitution and a
here-document past the close at each. Everything except the two warning lines and
their order is already correct, so nothing observable to a script is wrong.

**Fixed 2026-08-06** by doing the nested gather where bash does it, at the nested
`)`. `read_balanced_inner`'s `nested` stack now carries, alongside each nested
level's depth, the length `pending` had when that level opened; at the level's
`)` the entries from that mark on are exactly the ones it declared and did not
delimit, and they are handed to `gather_ahead` there and then — with the count as
`own`, so the warning is raised at that moment, on the line the reader is
actually on. `gather_ahead` records the read-ahead in `hd_ahead`, and
`fetched_line()` already prefers `hd_ahead.line`, so the outer scan's own warning
at its `)` — and the `here-document … delimited by end-of-file` that follows a
body running out — pick up the advanced line for free.

Two things the plan above got wrong, both in the direction of less work:

- **The raw text is not awkward.** The body is spliced in *at the nested `)`*,
  ahead of the paren the scan is about to copy, so the re-lex of the outer body
  finds the nested here-document **inline** — `$(cat <<B` / `bbb` / `B` / `)` —
  exactly as if the source had been written that way. No splitting of
  `consume_subst_heredoc_bodies`, and the two gathers stay in reader order.
- **No suppression is needed.** Because the splice is inline, the re-lex never
  reaches a `)` with anything pending, so it never calls `gather_ahead` and has
  nothing to warn about. The `Lexer::paren_body` flag was not touched.

One real subtlety did turn up: a gathered body whose last line ends the input has
no newline of its own to copy, so the delimiter and the `)` would be spliced into
`B)` and the re-lex would look for a delimiter it can no longer find — reported
as `unexpected EOF while looking for matching ')'`. The splice therefore ends the
copy with a newline if the body did not. (This never bit the outer gather, which
happens last and has no `)` left to copy after it.)

**Verified:** `tests/corpus/a-here-document-past-two-closes-is-fetched-at-the-inner-one.sh`
(byte-for-byte, 11 shapes including three levels of nesting, two here-documents
at the inner level, a `case` and a `<( … )` inside the inner one, and two `eval`s
whose input runs out), plus two more cases in
`lexer::tests::a_substitution_gathers_a_here_document_from_past_its_close`.

**Found while measuring:** a here-document declared inside a *double-quoted*
nested substitution was not gathered at all — see
TD-OILS-CMDSUB-HEREDOC-INSIDE-A-QUOTED-NESTED-SUBSTITUTION below, fixed the same
day on top of this one.
