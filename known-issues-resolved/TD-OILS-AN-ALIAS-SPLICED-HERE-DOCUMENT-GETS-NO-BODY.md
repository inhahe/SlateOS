### TD-OILS-AN-ALIAS-SPLICED-HERE-DOCUMENT-GETS-NO-BODY. A `<<` that arrives through an alias loses its redirection entirely — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `expand_aliases_tracked` /
`expand_aliases_inner`, which lex each alias replacement as a *text of its own*
(`AliasInput::src`), and `Lexer::collect_heredocs`, which can only collect a body
from the text the `<<` was lexed in.

**What.** A here-document operator written inside an alias value does nothing:

```
shopt -s expand_aliases
alias A="cat <<E"
A
body
E

bash: body
osh : line 4: body: command not found
      line 5: E: command not found          rc=127
```

The `<<` is not merely un-collected — it is gone. With the delimiter supplied by
the calling line instead, the shape is unmistakable:

```
alias B="cat <<"
B E2          bash: reads the following lines as E2's body
              osh : runs `cat E2` — the operator dropped, the delimiter an argument
```

`alias A="cat <<E; echo tail"` shows the same from the other side: osh prints
`tail`, so the command did run, with `cat` reading nothing.

**Why.** bash expands an alias by pushing its value onto the *input stream*
(`push_string`, parse.y), so the `<<` is read by the same reader that will later
reach the newline and gather — the body comes off the calling line's input, which
is exactly where the user wrote it. osh lexes the replacement separately: the
`<<` is scanned in a text where there is no following newline to gather at and no
body to gather, so the pending here-document is discarded with the sub-lexer.

**Proper fix.** The alias pass has to hand a `<<` it emits back to the outer
text's pending list rather than resolving it in the replacement's own lexer —
i.e. `PendingHeredoc` must survive the text boundary, carrying the outer reader
position, so the outer newline collects it.

**As fixed.** The pending record does not survive the boundary; it is *replaced*
by a gather run out of the middle of the real input, which is a closer model of
what bash does. Reading bash settled the shape:

* `push_string` (parse.y:2694) makes the replacement the current
  `shell_input_line`, so the `<<` is read by the same reader that will reach the
  calling line's newline and joins the same `redir_stack`.
* The **body** is not read by that reader at all. `gather_here_documents` calls
  `make_here_document` (make_cmd.c:590) → `read_secondary_line` → `read_a_line`
  (parse.y:2080), and `read_a_line` takes characters from `yy_getc()` —
  `bash_input.getter`, the underlying *file*. Nothing on that path consults
  `shell_input_line`. So the body is the next physical line of the file, a body
  written inside the alias value is **not a body** (the reader reaches that text
  later, through the ordinary door, and runs it as commands), and an alias `<<`
  and a `<<` of the calling line share one moving cursor in declaration order.
* `make_here_document` bumps `line_number` once per line handed over, the
  delimiter's included, and the gather fires the moment the reader sees a
  newline. When the value itself contains one, that is the newline — so the rest
  of the value *and* the rest of the calling line are numbered from where the
  gather stopped. With no newline in the value the gather waits for the calling
  line's own and the whole line keeps its number.

The code:

* `Lexer::defer_heredocs` + `tokenize_alias_body` — the alias pass lexes with no
  newline gather at all, so a `<<` in a value emits an empty placeholder.
  `expand_aliases_inner` records each such placeholder in `AliasExpansion::
  heredocs`, which becomes `IncrementalParser::work_alias_heredocs`.
* `gather_heredocs_at` + `Lexer::at_offset` — collect bodies for a list of
  `HeredocWant`s starting at an arbitrary character offset, returning the tokens,
  the lines each took, the end offset, the warnings and the continuations.
* `IncrementalParser::gather_alias_heredocs` — re-collects **the whole physical
  line's** here-documents in declaration order (an alias `<<` before a line's own
  shifts every later body, so the ones the first lex did collect were collected
  from the wrong place), stamps `work_heredoc_lines` / `orig_heredoc_lines`,
  re-keys the warnings onto the line's newline, fixes `orig_conts` and
  `orig_ends`, applies the post-gather line number, and then
  `relex_tail_from(keep, off)` re-lexes the input past the bodies — because those
  lines are not commands after all, while the already-expanded line that
  introduced them has to stay exactly as it is.
* `IncrementalParser::rebuild` drives this as a bounded multi-pass loop (each
  pass settles one physical line; the settled prefix of `orig` never changes, so
  `AliasFills` replays by index). Because the gathering runs *ahead* of the parse,
  over lines no command has run before yet, a later `alias` can invalidate it —
  so every `rebuild` first undoes the previous one's gathers by re-lexing the
  unparsed tail (`alias_gathered`), and sees the input as bash's reader would with
  the table it has now.
* `heredoc_gather` no longer skips an alias-spliced `<<`: it reads the count from
  `work_heredoc_lines` when there is no `orig` slot.

**Pinned by** the corpus case
`an-alias-spliced-here-document-takes-its-body-from-the-real-input.sh` (which
fails on its very first probe without the fix, and diverges in almost every line
of stdout and stderr thereafter) and the unit test
`an_alias_spliced_here_document_takes_its_body_from_the_real_input`.

**Left behind:**
TD-OILS-AN-ALIAS-SPLICED-HERE-DOCUMENT-TAKES-NO-DELIMITER-FROM-THE-CALLING-LINE,
the other half of the second shape above (since fixed, same day).
