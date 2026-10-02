### TD-OILS-A-PENDING-HERE-DOCUMENT-IS-NOT-GATHERED-WHEN-A-TOP-LEVEL-LIST-IS-REDUCED. Two symptoms, one missing rule — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/parser.rs` — `Parser::reader_line` /
`Parser::reader_echo`, and the lexer/parser split that puts here-document
collection entirely in `userspace/oils/src/lexer.rs`.

**What.** bash gathers a pending here-document from *three* places, not one. The
obvious one is the newline token (`read_token`, parse.y:3450). The two osh does
not model are yacc reductions:

```
simple_list:	simple_list1
			{ ...  if (need_here_doc) gather_here_documents ();  ... }
```

(parse.y:1217, and the `&`/`;` variants at 1235/1250), plus the same in
`compound_list`. So an offending token that is merely the *lookahead* for a
top-level `simple_list1 → simple_list` reduction gets the here-document
collected before the error is reported — and collection moves bash's reader.

Two divergences fall out of that, and they are the same bug seen twice.

**Symptom 1 — the error is blamed too early.** `make_here_document`
(make_cmd.c:621) does `line_number++` once per line it reads, delimiter line
included, so after collection the reader sits on the delimiter line:

```text
cat <<E(          bash: line 3: syntax error near unexpected token `('
body                    line 3: `cat <<E('
E                 osh:  line 1: … (same message, same echoed text)
echo tail
```

| source | bash | osh |
|---|---|---|
| `cat <<E(` / `body` / `E` / `echo tail` | 3 | 1 |
| the same with three body lines | 5 | 1 |
| `echo one` first | 4 | 2 |
| `cat <<E;;` / `body` / `E` / `echo tail` | 3 | 1 |
| `cat <<E` / `body` / `E` / `cat <<F(` / `b` / `F` | 6 | 4 |

Note the **echoed line does not move with the number** — bash prints
`cat <<E(`, which is line 1, under a "line 3:" prefix. `read_a_line`
(parse.y:2080) reads here-document bodies into a buffer of its own and never
replaces `shell_input_line`, so only `line_number` advances. osh derives the
echo from the reported number (`nth_source_line` in `Shell::…`, interp.rs
~45211), so moving the number alone would echo the wrong text; `ParseError::echo`
already exists to override it and is what should carry the token's own line.

**Symptom 2 — a pending here-document's warning is lost.** If an unterminated
construct *after* the `<<` on the same line swallows the rest of the input, the
here-document never gets a body, and the same reduction warns about it:

```text
cat <<E "         bash: line 1: unexpected EOF while looking for matching `"'
body                    line 4: warning: here-document at line 4 delimited by
E                             end-of-file (wanted `E')
                  osh:  the error only
```

Both numbers in the warning are the EOF line, collection having "begun" there.
Two pending here-documents give two warnings, in declaration order
(`cat <<E; cat <<F "` / `a` / `E` / `b` / `F` warns for `E` then `F`). Holds for
every `parse_matched_pair` construct — `"`, `'`, `` ` ``, `${`, `$((` — and for
`<<-` and a quoted delimiter alike.

**What proves it is one rule, not two.** The reduction only fires at top level,
and *both* symptoms vanish together the moment the command is inside a compound:

| source | bash |
|---|---|
| `cat <<E(` / `body` / `E` / `echo t` | line **3** |
| `{ cat <<E(` / … | line **1** |
| `if cat <<E(` / … | line **1** |
| `while cat <<E(` / … | line **1** |
| `cat <<E "` / `body` / `E` | error **+ warning** |
| `if cat <<E "` / `body` / `E` | error, **no warning** |

`$( … )` is exempt from both, because `parse_comsub` parses the body in a nested
reader rather than letting `parse_matched_pair` swallow it: `cat <<E $(x` /
`body` / `E` gives the error and nothing else.

**Proper fix.** The obstacle is architectural: osh lexes the whole input first,
so here-document collection (and its warnings) happens in the lexer, with no
parser reduction to hang off. The rule needs the parser's answer to "is this
token the lookahead of a top-level list reduction?", which is exactly
"`Parser::parse_list` is about to finish an item and the nesting depth is zero".

So: have the lexer record, per pending here-document, the reader position
collection *would* leave the reader at (it already computes both numbers for
`ReaderWarning::HeredocEof`), and carry the still-pending ones on the parked
`LexError`. Then the parser, at the point where it stamps a diagnostic
(`parse_tokens` and `IncrementalParser`, parser.rs:971 and :1476), adds the
here-document advance to `reader_line()` and pins `reader_echo()` to the token's
own line — but only when the error is at top level, and emits the pending
warnings under the same condition.

**Symptom 1, as fixed (2026-08-06).** Three pieces, one per file:

* `lexer.rs` — `collect_heredocs` now notes, per body it collects, the input
  lines the collection consumed, keyed by the placeholder token's index. The
  count is taken straight off the reader: newlines in the span the collection
  advanced over, plus one if it ended mid-line. That is precisely bash's advance,
  because `make_here_document` (make_cmd.c:621) bumps `line_number` once per line
  `read_a_line` hands it (delimiter line included) and `read_a_line`
  (parse.y:2080) bumps it again for each `\<newline>` it joins away — the two
  together being the physical lines the body occupied. `Tokenized` carries them
  out in a `heredoc_lines: Vec<u32>` parallel to `toks`. The `$( … )` body path
  (`consume_subst_heredoc_bodies`) is deliberately left alone: bash exempts it.
* `parser.rs` — `Parser::depth` counts nested command lists, raised in exactly
  two places (`parse_program` and `parse_case_body`, each split into a wrapper
  and an `_inner`) because `IncrementalParser::next_unit` drives `parse_item`
  directly, so depth 0 *is* the top level. It is unwound only on success: a
  failing parse must be stamped with the depth the error was raised at.
* `parser.rs` — `IncrementalParser::heredoc_gather` sums the recorded counts of
  the here-documents still pending when the error was raised (scanning back from
  the offending token to the last newline, which is what "pending" means), and
  only at depth 0. The stamping site adds that to `reader_line()` and, when it is
  non-zero, pins the echo to the offending token's own physical line with
  `tok_line_text` — sliced out of `src` via `orig_offsets`, so no `LineMap`
  renumbering can corrupt it.

The "only ones declared *before* the offending token" refinement is what
separates `cat <<A <<B(` (line 5) from `cat <<A( <<B` (line 3) on the same five
lines of input.

**Pinned by**
`tests/corpus/a-pending-here-document-is-gathered-before-the-token-that-ends-the-list-is-blamed.sh`
(21 probes; fails on its first without the fix) and the unit test
`a_pending_here_document_is_gathered_before_a_top_level_list_is_blamed`.

**Symptom 2, as fixed (2026-08-06).** The other half of the same reduction, and
the same three files:

* `lexer.rs` — a new `UngatheredHeredoc { delim, line, op_offset }`, collected in
  `tokenize_deferred`'s error path from whatever is still in `pending_heredocs`,
  *before* the truncation that removes the `<<`'s whole line (which is the only
  other trace of it). `line` is `Lexer::eof_line()` for both of the warning's
  numbers, because the reader had already run to the end of the input looking for
  the unclosed construct's close. Carried out on `Tokenized::ungathered`.
* `lexer.rs` — `read_subst_body` now sets a `heredocs_forgotten` flag when it
  fails, and the flag suppresses the whole record. That is bash's `parse_comsub`
  zeroing `need_here_doc` (parse.y:4133) and never restoring it on the
  `EOF_Reached` path, which is why `$( … )` — and process substitution, which
  bash reads with the same function — is the one construct that loses them. The
  `$((`-backtracks-to-a-substitution site deliberately calls
  `read_balanced_inner` directly instead: `parse_comsub` returns into
  `parse_matched_pair` with `P_ARITH` (parse.y:4103) *above* the zeroing, so
  `cat <<E $((`, `cat <<E $(( 1 +` and `cat <<E $(( echo a )` all warn.
* `parser.rs` — `IncrementalParser::ungathered_warnings`, called on exactly the
  condition (`lex_err_now`) that already lifts the ordinary warning frontier, and
  queueing onto a new `post_error_warnings` channel because these print *below*
  the diagnostic: the lexer had already reported the unclosed construct by the
  time yacc's default reductions reached `simple_list`. The top-level gate cannot
  use `Parser::depth` — the truncation removes the `<<`'s line, so the parser
  stands at depth 0 whatever enclosed it — so it re-parses the source in front of
  the `<<` instead: the operator was at the top level exactly when that prefix is
  a complete program of its own. `{ cat ` is not one, `echo one` + `cat ` is, and
  `) cat ` is not either — which is right, since bash abandons the parse at the
  `)` and never reads far enough to have a here-document pending.
* `interp.rs` — the two channels share one renderer (`reader_warning_msg`); the
  post-error one is drained in the `Err(e)` arm after `format_parse_error`.

**Pinned by** the two corpus cases
`a-pending-here-document-is-gathered-before-the-token-that-ends-the-list-is-blamed.sh`
(21 probes) and
`a-here-document-the-reader-never-reached-is-warned-about-below-the-error.sh`
(32 probes), and the unit tests
`a_pending_here_document_is_gathered_before_a_top_level_list_is_blamed` and
`a_here_document_the_lex_never_reached_is_warned_about_after_the_error`. Both
corpus cases fail on their first probe without the fix.

**Left behind:**
TD-OILS-AN-ALIAS-SPLICED-HERE-DOCUMENT-GETS-NO-BODY,
TD-OILS-A-STRAY-PAREN-IN-A-CASE-ARM-IS-NOT-DIAGNOSED-WHERE-IT-STANDS and
TD-OILS-A-LINE-THAT-FAILS-TO-LEX-IS-NOT-REPORTED-AS-A-LINE, all found while
measuring shapes for the corpus cases.
