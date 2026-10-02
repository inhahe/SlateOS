### TD-OILS-A-SUBSTITUTION-IN-TEXT-NO-PARSER-READ-WAS-PARSED-AT-PARSE-TIME. A `$( … )` in a here-document body was re-printed, and its errors were the script's — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/lexer.rs` (`Lexer::subst_kind`, `read_dollar`'s
`$(` arm, `read_subst_body`), `src/ast.rs` (`CmdSubBody::Unread`),
`src/parser.rs` (`seg_to_parts`, `dquote_word_from_source`), `src/unparse.rs`
(`part_src`, `attach_comsub_tails`), `src/interp.rs`
(`command_sub_body_inner`, `comsub_reparse_fail`, `Shell::prompt_expanding`).
Corpus: `tests/corpus/a-here-document-body-is-not-a-word-the-parser-read.sh`.

**What.** osh lexed a here-document body as though the body were a word, so a
`$( … )` in it got the ordinary eager parse: its body was re-printed for
`declare -f`, and a syntax error in it was a *parse* error that killed the
script. bash does neither.

**Why.** bash's reader never calls `read_token_word` over a here-document body.
`read_secondary_line` collects the lines and `make_here_document`
(make_cmd.c:621) takes them as text, so `parse_matched_pair` never runs, the
delimiter stack is empty for the body, and nothing in it is translated at parse
time (`expand_word_internal` takes the ANSI-C branch only when
`(quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) == 0`). Every substitution in the
body is therefore found only at *expansion* time, by `extract_command_subst`
(subst.c:1278) — and because `param_expand`'s `case LPAREN` (subst.c:10570)
sets `t_index = zindex + 1`, i.e. *past* the `(`, that takes the
`xparse_dolparen` branch rather than `extract_delimited_string`.

The same holds for every other string the shell expands as a double-quoted run
without a parser having read it as a word: `PS4` before an xtrace line, and
`${x@P}`.

**Fix, as landed.** A third `$( … )` spelling, `CmdSubBody::Unread`, carrying
the **source** rather than a re-print:

* `Lexer::here_text` — already set by `scan_heredoc_segs`, `lex_dquote_body`
  and `lex_operand_in_dquote` — now also selects the body kind, and is no
  longer cleared on the way into `read_subst_body`. Keeping it set is what made
  the printback verbatim: the balanced read is only finding the `)`, so it must
  not translate or re-quote what it passes over. `arith_comsubs` is likewise not
  recorded for such a scan, there being no parse to re-print.
* `declare -f` prints `$(` + the source + `)`.
* At expansion time the body is re-read exactly as a `Parsed` body's re-print is
  — same `comsub_reparse_fail`, since in bash it is literally the same
  `xparse_dolparen` call over whatever text the word holds — so a body that will
  not parse is a runtime `command substitution:` diagnostic and a
  `jump_to_top_level (DISCARD)` that leaves `$?` at 1 and lets the script go on.
* Two line numbers, one apart. The extent-finding read goes through
  `parse_string`, which does `push_stream (0)` and **omits**
  `parse_and_execute`'s `line_number--` (evalstring.c:329); the child that
  actually runs the body goes through `parse_and_execute`, which does not omit
  it (a non-interactive `command_substitute` passes no `SEVAL_RESETLINE` —
  subst.c:6986). So the run is numbered from the shell's *current* line and the
  extent read one higher. Measured on `hd4`, `ha`, `s3`, `hb`, `t1`, `t2`: the
  base is the owning simple command's line and is independent of where in the
  body the substitution sits.
* `Shell::prompt_expanding` models `no_longjmp_on_fatal_error`, which
  `expand_prompt_string` (subst.c:4459) raises around the whole expansion and
  `extract_command_subst` (subst.c:1289) passes down as `SX_NOLONGJMP`. All that
  flag reaches is the one guard around the raise in `xparse_dolparen`
  (parse.y:4330) — so a failing body in a `PS1`/`PS4`/`${x@P}` is *reported* and
  then handed to `command_substitute` anyway, whose child reports it a second
  time one line lower. Result: two diagnostics, an empty expansion, and `$?`
  untouched. The flag is inherited by `clone_for_subshell`, as a fork inherits
  every global.
* `dquote_word_from_source` now runs `attach_comsub_tails`. A word the shell
  assembles from a *value* has no re-print, but `expand_word_internal` is still
  walking its string, so `extract_command_subst` still gets the whole remainder
  and a failing body still echoes it.
* A third `parser::Quoting`, `Unread`. Unread-ness is *not* a property of the
  operand text — `${x:-$'a\x2Cb'}` reaches `lex_operand_in_dquote` identically
  from `"…"` and from a here-document body — so it has to be carried down from
  the read, exactly as `Dquote` already is. Setting `here_text` unconditionally
  in `lex_operand_in_dquote` instead cost a corpus case
  (`a-quoted-operands-quoting-is-the-quotes-it-sits-in`, the `locale` row:
  `"${nope:-$"hi"}"` must still translate, because there the operand *is* a word
  a parser read). `read_dollar`'s `$'…'`/`$"…"` arms are gated on
  `!in_dquote && !here_text` — two different reasons for the same answer.

**Left behind:** TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING,
TD-OILS-A-BACKQUOTE-IN-A-HERE-DOCUMENT-BODY-IS-NUMBERED-FROM-THE-BODY and
TD-OILS-AN-UNTERMINATED-DOLLAR-PAREN-IN-A-HERE-DOCUMENT-BODY-IS-A-PARSE-ERROR,
all below.
