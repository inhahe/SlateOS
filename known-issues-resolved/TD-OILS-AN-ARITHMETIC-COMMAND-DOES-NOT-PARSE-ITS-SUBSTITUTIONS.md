### TD-OILS-AN-ARITHMETIC-COMMAND-DOES-NOT-PARSE-ITS-SUBSTITUTIONS. `(( 1 + $(fi) ))` should be a syntax error — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_arith_body`, which is
the only `P_ARITH`-equivalent scan that does not hand its collected
`CmdSubSpan`s back; `Tok::ArithCmd(Str)` carries none, so `parser.rs`'s
`Command::Arith(raw)` never sees them.

**What.** `parse_arith_cmd` reads a `(( … ))` body with
`parse_matched_pair (0, '(', ')', &ttoklen, P_ARITH)` (parse.y:4519–4530) — the
same scan `$(( … ))` gets. So everything `P_ARITH` implies applies: a nested
`$( … )` is **parsed where it is read**, its syntax error is the enclosing
unit's, and what is kept is the parse re-printed rather than the source.

osh's `read_arith_body` calls the same `read_opaque_span(…, command = false)`
that does the collecting, so the spans are produced — and then dropped on the
floor, because `read_arith_body` returns only the text. Neither half happens:

```text
(( 1 + $(fi) ))
  bash: syntax error near unexpected token `fi'   /  `(( 1 + $(fi) ))'   rc=1
  osh:  the substitution's own error, then `((: 1 +  : syntax error…'

if false; then (( 1 + $(fi) )); fi
  bash: the same fatal error, branch untaken       rc=1
  osh:  rc=0, silence

for ((i=$(fi);;)); do :; done       same shape as the first
(( ${x!} + $(echo a>&2) ))
  bash: ` ${x!} + $(echo a 1>&2) : bad substitution'
  osh:  ` ${x!} + $(echo a>&2) : bad substitution'   (source, not re-print)
```

There is a second, latent consequence: the dropped spans are not dropped, they
are *left in* `Lexer::arith_comsubs`. Every other producer swaps that list out
for the duration; `read_arith_body` does not, so a `(( … ))` leaves entries
behind and the `None` (nested-subshell) rewind does not remove them either.
Nothing consumes them today — each `$((` scan saves and restores the outer list
rather than reading it — so this is not currently reachable, but it is one
change away from being a real cross-token leak.

**Found by** the probe matrix for
TD-OILS-AN-ARITHMETIC-STRING-NAMES-ITS-COMMAND-SUBSTITUTION-AS-WRITTEN,
2026-08-07.

**Fixed 2026-08-07** exactly as the plan above described.
`Lexer::read_arith_body` is now a two-line wrapper that `mem::take`s
`self.arith_comsubs`, runs the old body as `read_arith_body_inner`, and swaps the
outer list back — so the spans belong to *this* scan and the latent cross-token
leak is closed with the same change. It returns `(Option<Str>,
Vec<CmdSubSpan>)`; `Tok::ArithCmd` widened to `ArithCmd(Str, Vec<CmdSubSpan>)`,
`map_lines` renumbers the nested spans like every other `P_ARITH` producer, and
`parser.rs` runs them through `parse_arith_comsubs` + `splice_reprints` in both
`parse_command` (the `(( … ))` command) and `parse_for` (the `for (( … ))`
header).

Two details settled by measurement rather than by reading:

- **The `for` header is spliced *before* it is cut on `;`.** The ranges index
  into the one buffer the single `P_ARITH` scan built, so they only mean
  something while that buffer is whole. bash agrees: parse.y:4519–4530 collects
  the whole header, and `ARITH_FOR_EXPRS` splits it afterwards. (A recorded
  guess that section 1 keeps a leading space and section 2 does not was
  **falsified** — bash trims leading whitespace from all three sections, while
  `(( … ))` proper does not. The corpus case records the measurement.)
- **The rewind path may drop the spans.** When the second `)` is not adjacent
  the text is re-read as `( ( … ) )`, and the ordinary command parser parses
  each body a second time — so a body that does not parse is still fatal, and
  one that does is re-printed by `unparse` from the node it produced. Nothing is
  lost. That bash really does parse the body *before* testing adjacency is
  measured, not assumed: `((echo $(fi) ) )` is a fatal syntax error at `fi`.

Corpus case:
`tests/corpus/an-arithmetic-command-carries-its-command-substitution-re-printed.sh`.
